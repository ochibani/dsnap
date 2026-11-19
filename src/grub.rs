use crate::{chr_delete, deploy_recovery, detect_distro, get_aux_tmp, get_part, get_recovery_aux_tmp,
            get_recovery_tmp, get_tmp, post_transactions, prepare};

use chrono::{Local, NaiveDateTime};
use nix::mount::{MntFlags, mount, MsFlags, umount2};
use std::fs::{copy, read_to_string, DirBuilder, File};
use std::io::{Error, ErrorKind, Read, Write};
use std::path::Path;
use std::process::Command;
use tempfile::{NamedTempFile, TempDir};
use walkdir::WalkDir;

fn chroot_command(root: &str, program: &str, args: &[&str]) -> Result<(), Error> {
    let status = Command::new("chroot").arg(root).arg(program).args(args).status()?;
    if !status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to run {} in {}.", program, root)));
    }
    Ok(())
}

// Replace a file atomically so interrupted writes cannot leave it truncated.
fn atomic_write(path: &Path, content: &str) -> Result<(), Error> {
    let parent = path.parent().unwrap_or_else(|| Path::new("/"));
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(content.as_bytes())?;
    tmp.as_file().sync_all()?;
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(tmp.path(), meta.permissions())?;
    }
    tmp.persist(path).map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    Ok(())
}

// Count unbalanced braces outside quotes and comments. GRUB menuentry bodies
// can contain nested braces, so the first "}" is not necessarily the end.
fn brace_delta(line: &str) -> i32 {
    let mut delta = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && (in_single || in_double) {
            escaped = true;
            continue;
        }
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => break,
            '{' if !in_single && !in_double => delta += 1,
            '}' if !in_single && !in_double => delta -= 1,
            _ => {}
        }
    }
    delta
}

fn is_menuentry_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.starts_with('#') && trimmed.starts_with("menuentry")
}

fn extract_first_menuentry(config: &str) -> Result<String, Error> {
    let mut in_section = false;
    let mut started = false;
    let mut depth = 0;
    let mut block: Vec<&str> = Vec::new();

    for line in config.lines() {
        if !in_section {
            if line.contains("BEGIN /etc/grub.d/10_linux") {
                in_section = true;
            }
            continue;
        }
        if !started {
            if is_menuentry_start(line) {
                started = true;
                depth = brace_delta(line);
                block.push(line);
                if depth <= 0 {
                    return Ok(block.join("\n"));
                }
            }
            continue;
        }
        block.push(line);
        depth += brace_delta(line);
        if depth <= 0 {
            return Ok(block.join("\n"));
        }
    }
    Err(Error::new(ErrorKind::NotFound,
                   "Could not find a menuentry in the 10_linux GRUB section."))
}

fn is_recovery_menuentry(line: &str) -> bool {
    is_menuentry_start(line)
        && (line.contains("'Recovery Snapshot'")
            || line.contains("\"Recovery Snapshot\"")
            || line.contains("'Recovery Mode'")
            || line.contains("\"Recovery Mode\""))
}

fn remove_menuentries(config: &str, predicate: impl Fn(&str) -> bool) -> String {
    let lines: Vec<&str> = config.lines().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_menuentry_start(line) && predicate(line) {
            let mut depth = brace_delta(line);
            i += 1;
            while i < lines.len() && depth > 0 {
                depth += brace_delta(lines[i]);
                i += 1;
            }
            continue;
        }
        out.push(line);
        i += 1;
    }
    let mut result = out.join("\n");
    if config.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn remove_recovery_entries(config: &str) -> String {
    let cleaned = config.replace("### END /etc/grub.d/41_custom ###", "");
    remove_menuentries(&cleaned, is_recovery_menuentry)
}

// Mount the boot subvolume for the duration of a GRUB write operation.
// Drop always unmounts, including on early returns, so no mount is leaked.
struct BootMount {
    _dir: TempDir,
    mounted: bool,
}

impl BootMount {
    fn new(part: &str) -> Result<Self, Error> {
        let dir = TempDir::new_in("/.snapshots/tmp")?;
        #[cfg(feature = "btrfs")]
        {
            mount(Some(part), dir.path().as_os_str(),
                  Some("btrfs"), MsFlags::empty(), Some("subvol=@boot_linux".as_bytes()))?;
            Ok(Self { _dir: dir, mounted: true })
        }
        #[cfg(not(feature = "btrfs"))]
        {
            let _ = dir;
            Err(Error::new(ErrorKind::Unsupported,
                           "GRUB boot operations require the btrfs feature."))
        }
    }

    fn path(&self) -> &Path {
        self._dir.path()
    }
}

impl Drop for BootMount {
    fn drop(&mut self) {
        if self.mounted {
            if let Err(e) = umount2(self._dir.path(), MntFlags::MNT_DETACH) {
                eprintln!("Failed to unmount boot partition at {}: {}",
                          self._dir.path().display(), e);
            }
        }
    }
}

// Delete old grub.cfg
pub fn delete_old_grub_files(grub: &str) -> Result<(), Error> {
    let bak_path = Path::new(grub).join("BAK");
    for entry in WalkDir::new(bak_path) {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.to_str().unwrap_or("").contains("grub.cfg") {
            // BAK files are named grub.cfg.YYYYmmdd-HHMMSS; skip anything
            // that does not carry a parseable timestamp extension
            let file_ext = match path.extension().and_then(|e| e.to_str()) {
                Some(ext) => ext,
                None => continue,
            };
            let file_create_time = match NaiveDateTime::parse_from_str(file_ext, "%Y%m%d-%H%M%S") {
                Ok(t) => t,
                Err(_) => continue,
            };
            let cutoff_time = Local::now().naive_local().signed_duration_since(file_create_time);
            if cutoff_time.num_days() >= 30 {
                std::fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

// Get Grub path
pub fn get_grub(snapshot: &str) -> Result<String, Error> {
    let boot_dir = format!("/.snapshots/boot/boot-{}", snapshot);
    let mut grub_dirs: Vec<String> = Vec::new();
    for entry in WalkDir::new(boot_dir) {
        let entry = entry.map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        if entry.file_type().is_dir() {
            if let Some(dir_path) = entry.path().file_name() {
                if dir_path == "grub" || dir_path == "grub2" {
                    let rel = entry.path()
                        .strip_prefix(format!("/.snapshots/boot/boot-{}", snapshot))
                        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?
                        .to_string_lossy()
                        .into_owned();
                    grub_dirs.push(rel);
                }
            }
        }
    }
    grub_dirs.sort();
    grub_dirs.into_iter().next().ok_or_else(|| {
        Error::new(ErrorKind::NotFound,
                    format!("Could not find grub directory for snapshot {}.", snapshot))
    })
}

// Switch between /recovery-tmp deployments
pub fn switch_recovery_tmp(snapshot: &str, target_snapshot: &str) -> Result<(), Error> {
    let grub = get_grub(snapshot)?;
    let part = get_part()?;
    let boot = BootMount::new(&part)?;

    // Swap deployment subvolumes: deploy <-> deploy-aux
    let source_dep = get_recovery_tmp();
    let target_dep = get_recovery_aux_tmp(&source_dep);
    let boot_location = boot.path().to_string_lossy();

    // Read the contents of the file into a string
    let target_grub = get_grub(target_snapshot)?;
    let grub_cfg = format!("{}/{}/grub.cfg", boot_location, target_grub);
    let src_file_path = format!("/.snapshots/rootfs/snapshot-{}/boot/{}/grub.cfg", source_dep,grub);
    let mut sfile = if Path::new(&src_file_path).is_file() {
        File::open(&src_file_path)?
    } else {
        File::open(format!("/.snapshots/rootfs/snapshot-{}/boot/{}/grub.cfg", target_dep,grub))?
    };
    let mut source_cfg = String::new();
    sfile.read_to_string(&mut source_cfg)?;
    let gconf = extract_first_menuentry(&source_cfg)?;

    // Remove old recovery entries before appending the current one.
    let mut contents = String::new();
    File::open(&grub_cfg)?.read_to_string(&mut contents)?;
    atomic_write(Path::new(&grub_cfg), &remove_recovery_entries(&contents))?;

    // Change recovery tmp
    let updated_line = if let Some(first_quote_index) = gconf.find('\'') {
        if let Some(rel) = gconf[first_quote_index + 1..].find('\'') {
            let second_quote_index = rel + first_quote_index + 1;
            gconf.replace(&gconf[first_quote_index + 1..second_quote_index], "Recovery Snapshot")
        } else {
            gconf.clone()
        }
    } else {
        gconf.clone()
    };
    // Replace the full snapshot token. "recovery-deploy" is a substring
    // of "recovery-deploy-aux", so prefer the longer/more specific match
    // to avoid producing "snapshot-recovery-deploy-aux-aux"
    let modified_cfg_contents = if updated_line.contains(&format!("@.snapshots_linux/rootfs/snapshot-recovery-deploy-aux")) {
        updated_line.replace("@.snapshots_linux/rootfs/snapshot-recovery-deploy-aux",
                             &format!("@.snapshots_linux/rootfs/snapshot-{}", target_dep))
    } else if updated_line.contains(&format!("@.snapshots_linux/rootfs/snapshot-recovery-deploy")) {
        updated_line.replace("@.snapshots_linux/rootfs/snapshot-recovery-deploy",
                             &format!("@.snapshots_linux/rootfs/snapshot-{}", target_dep))
    } else if updated_line.contains(&format!("@.snapshots_linux/rootfs/snapshot-{}", source_dep)) {
        updated_line.replace(&format!("@.snapshots_linux/rootfs/snapshot-{}", source_dep),
                             &format!("@.snapshots_linux/rootfs/snapshot-{}", target_dep))
    } else {
        let src_tmp = if updated_line.contains("deploy-aux-secondary") {
            "deploy-aux-secondary"
        } else if updated_line.contains("deploy-aux") && !updated_line.contains("secondary") {
            "deploy-aux"
        } else if updated_line.contains("deploy-secondary") {
            "deploy-secondary"
        } else {
            "deploy"
        };

        updated_line.replace(&format!("@.snapshots_linux/rootfs/snapshot-{}", src_tmp),
                             &format!("@.snapshots_linux/rootfs/snapshot-{}", target_dep))
    };

    // Write the modified contents back to the file
    let mut cfg = String::new();
    File::open(&grub_cfg)?.read_to_string(&mut cfg)?;
    cfg.push_str(&modified_cfg_contents);
    atomic_write(Path::new(&grub_cfg), &cfg)?;

    Ok(())
}

// Switch between /tmp deployments
pub fn switch_tmp(snapshot: &str, secondary: bool, reset: bool) -> Result<String, Error> {
    let grub = get_grub(snapshot)?;
    let part = get_part()?;
    let rec_tmp = get_recovery_tmp();
    let boot = BootMount::new(&part)?;

    // Swap deployment subvolumes: deploy <-> deploy-aux
    let source_dep = get_tmp();
    let source_grub = get_grub(&source_dep)?;
    let target_dep = get_aux_tmp(source_dep.to_string(), secondary);
    let excode = Command::new("cp").args(["-r", "--reflink=auto"])
                      .arg(format!("/.snapshots/rootfs/snapshot-{}/boot/{}", target_dep,grub))
                      .arg(format!("{}", boot.path().to_string_lossy()))
                      .status()?;
    if !excode.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to copy the GRUB image to the boot partition."));
    }

    // Update fstab for new deployment
    let fstab_file = format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", target_dep);
    // Read the contents of the file into a string
    let mut contents = String::new();
    let mut file = File::open(&fstab_file)?;
    file.read_to_string(&mut contents)?;
    let modified_boot_contents = contents.replace(&format!("@.snapshots_linux/boot/boot-{}", source_dep),
                                                  &format!("@.snapshots_linux/boot/boot-{}", target_dep));
    let modified_etc_contents = modified_boot_contents.replace(&format!("@.snapshots_linux/etc/etc-{}", source_dep),
                                                               &format!("@.snapshots_linux/etc/etc-{}", target_dep));
    let modified_var_contents = modified_etc_contents.replace(&format!("@.snapshots_linux/var/var-{}", source_dep),
                                                               &format!("@.snapshots_linux/var/var-{}", target_dep));
    let modified_rootfs_contents = modified_var_contents.replace(&format!("@.snapshots_linux/rootfs/snapshot-{}", source_dep),
                                                                 &format!("@.snapshots_linux/rootfs/snapshot-{}", target_dep));
    // Write the modified contents back to the file
    atomic_write(Path::new(&fstab_file), &modified_rootfs_contents)?;

    // Recovery GRUB configurations
    if !reset {
        for boot_location in [&boot.path().to_string_lossy()] {
            // Get old grub configurations
            let grub_path = format!("{}/{}/grub.cfg", boot_location, grub);
            let src_file_path = format!("/.snapshots/rootfs/snapshot-{}/boot/{}/grub.cfg", source_dep,source_grub);
            let mut sfile = File::open(&src_file_path)?;
            let mut source_cfg = String::new();
            sfile.read_to_string(&mut source_cfg)?;
            let gconf = extract_first_menuentry(&source_cfg)?;

            let mut contents = String::new();
            File::open(&grub_path)?.read_to_string(&mut contents)?;
            let mut cfg = remove_recovery_entries(&contents);
            cfg.push_str(&format!("\n\n{}", &gconf));
            atomic_write(Path::new(&grub_path), &cfg)?;

            // Add recovery mode
            if Path::new(&format!("/.snapshots/rootfs/snapshot-{}", rec_tmp)).try_exists()? {
                let updated_line = if let Some(first_quote_index) = gconf.find('\'') {
                    if let Some(rel) = gconf[first_quote_index + 1..].find('\'') {
                        let second_quote_index = rel + first_quote_index + 1;
                        gconf.replace(&gconf[first_quote_index + 1..second_quote_index], "Recovery Snapshot")
                    } else {
                        gconf.clone()
                    }
                } else {
                    gconf.clone()
                };

                // Change recovery tmp
                let gconf = updated_line.replace(&format!("@.snapshots_linux/rootfs/snapshot-{}", source_dep),
                                                 &format!("@.snapshots_linux/rootfs/snapshot-{}", rec_tmp));

                let mut contents = String::new();
                File::open(&grub_path)?.read_to_string(&mut contents)?;
                let mut cfg = remove_recovery_entries(&contents);
                cfg.push_str(&format!("\n\n{}", &gconf));
                atomic_write(Path::new(&grub_path), &cfg)?;
            } else {
                deploy_recovery(snapshot)?;
            }
        }
    } else {
        deploy_recovery(snapshot)?;
    }

    // Switch grub.cfg
    if grub == "grub" && Path::new(&format!("{}/grub2/grub.cfg", &boot.path().to_string_lossy())).try_exists()? {
        copy(format!("{}/{}/grub.cfg", &boot.path().to_string_lossy(), grub),
             format!("{}/grub2/grub.cfg", &boot.path().to_string_lossy()))?;
    } else if grub != "grub" && Path::new(&format!("{}/grub/grub.cfg", &boot.path().to_string_lossy())).try_exists()? {
        copy(format!("{}/{}/grub.cfg", &boot.path().to_string_lossy(), grub),
             format!("{}/grub/grub.cfg", &boot.path().to_string_lossy()))?;
    }

    Ok(target_dep)
}

// Update boot
pub fn update_boot(snapshot: &str, secondary: bool) -> Result<(), Error> {
    // Path to grub directory
    let grub = get_grub(snapshot)?;

    // Make sure snapshot does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot update boot as snapshot {} doesn't exist.", snapshot)));

    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        // Make sure snapshot is not in use by another dsnap process
        return Err(
            Error::new(
                ErrorKind::Unsupported,
                format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                        snapshot,snapshot)));

    } else {
        // Get tmp
        let tmp = get_tmp();
        let tmp = get_aux_tmp(tmp, secondary);

        // Partition path
        let part = get_part()?;

        // Prepare for update
        prepare(snapshot)?;

        let result = (|| -> Result<(), Error> {
            // Remove grub configurations older than 30 days
            if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}/boot/{}/BAK/", snapshot,grub)).try_exists()? && snapshot != "0" {
                delete_old_grub_files(&format!("/.snapshots/rootfs/snapshot-chr{}/boot/{}", snapshot,grub).as_str())?;
            }

            // Get current time
            let time = Local::now().naive_local();
            let formatted = time.format("%Y%m%d-%H%M%S").to_string();

            // Copy backup
            if snapshot != "0" {
                if !Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}/boot/{}/BAK/", snapshot,grub)).try_exists()? {
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/rootfs/snapshot-chr{}/boot/{}/BAK", snapshot,grub))?;

                }
                copy(format!("/.snapshots/rootfs/snapshot-chr{}/boot/{}/grub.cfg", snapshot,grub),
                     format!("/.snapshots/rootfs/snapshot-chr{}/boot/{}/BAK/grub.cfg.{}", snapshot,grub,formatted))?;
            }

            // Regenerate the GRUB configuration in the chroot.
            let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
            let boot_cfg = format!("/boot/{}/grub.cfg", grub);
            let mkconfig = if Path::new(&format!("{}/usr/sbin/grub2-mkconfig", root)).try_exists()? {
                "/usr/sbin/grub2-mkconfig"
            } else {
                "/usr/sbin/grub-mkconfig"
            };
            chroot_command(&root, mkconfig, &[&part, "-o", &boot_cfg])?;

            // Apply the same two substitutions the previous sed commands made.
            let cfg_path = format!("{}{}", root, boot_cfg);
            let mut cfg = read_to_string(&cfg_path)?;
            cfg = cfg.replace(&format!("snapshot-chr{}", snapshot),
                              &format!("snapshot-{}", tmp));
            let distro_name = detect_distro::distro_name(snapshot);
            if !distro_name.is_empty() {
                let labelled = format!("{} snapshot {}", distro_name, snapshot);
                cfg = cfg.replacen(&distro_name, &labelled, 1);
            }
            atomic_write(Path::new(&cfg_path), &cfg)?;

            // Finish the update
            post_transactions(snapshot)?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = chr_delete(snapshot);
            return Err(e);
        }
    }
    Ok(())
}
