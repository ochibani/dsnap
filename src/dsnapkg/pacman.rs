use crate::{audit_log, check_mutability, check_profile, chr_delete, chroot_exec, cp_reflink,
            cp_reflink_no_clobber, get_current_snapshot, get_tmp, grub, has_systemd,
            immutability_disable, immutability_enable, is_system_locked, is_system_pkg,
            post_transactions, prepare, profile_write, remove_dir_content, run_chroot,
            snapshot_config, sync_time, validate_package_specs, validate_service_names,
            validate_system_config_path};

use configparser::ini::{Ini, WriteOptions};
use rustix::path::Arg;
use std::fs::{File, metadata, OpenOptions, read_dir};
use std::io::{Error, ErrorKind, Read, Write};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use users::get_user_by_name;
use users::os::unix::UserExt;

// Check if AUR is setup right
pub fn aur_check(snapshot: &str) -> Result<bool, Error> {
    Ok(snapshot_config(snapshot)?.aur)
}

// Noninteractive update
pub fn auto_upgrade(snapshot: &str) -> Result<(), Error> {
    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(
            ErrorKind::NotFound, format!("Cannot upgrade as snapshot {} doesn't exist.", snapshot)));

    } else {
        // Required in virtualbox, otherwise error in package db update
        sync_time()?;

        // Prepare snapshot
        prepare(snapshot)?;

        // Avoid invalid or corrupted package (PGP signature) error
        let keyring_status = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                                   .args(["pacman", "-Sy", "--noconfirm", "archlinux-keyring"])
                                                   .status()?;
        if !keyring_status.success() {
            chr_delete(snapshot)?;
            return Err(Error::new(ErrorKind::Other, "Failed to refresh archlinux-keyring."));
        }

        if !aur_check(snapshot)? {
            // Use pacman
            let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                               .args(["pacman", "--noconfirm", "-Syyu"]).status()?;
            if excode.success() {
                post_transactions(snapshot)?;
                let mut file = OpenOptions::new().write(true)
                                                 .create(true)
                                                 .truncate(true)
                                                 .open("/.snapshots/dsnap/upstate")?;
                file.write_all("0 ".as_bytes())?;
                let mut file = OpenOptions::new().append(true)
                                                 .open("/.snapshots/dsnap/upstate")?;
                let date = Command::new("date").output()?;
                file.write_all(format!("\n{}", &date.stdout.to_string_lossy().as_str()?).as_bytes())?;
            } else {
                chr_delete(snapshot)?;
                let mut file = OpenOptions::new().write(true)
                                                 .create(true)
                                                 .truncate(true)
                                                 .open("/.snapshots/dsnap/upstate")?;
                file.write_all("1 ".as_bytes())?;
                let mut file = OpenOptions::new().append(true)
                                                 .open("/.snapshots/dsnap/upstate")?;
                let date = Command::new("date").output()?;
                file.write_all(format!("\n{}", &date.stdout.to_string_lossy().as_str()?).as_bytes())?;
                return Err(Error::new(ErrorKind::Other,
                                      "Failed to upgrade."));
            }
        } else {
            // Use paru if aur is enabled
            let args = format!("paru -Syyu --noconfirm");
            let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                               .args(["su", "aur", "-c", &args])
                                               .status()?;
            if excode.success() {
                post_transactions(snapshot)?;
                let mut file = OpenOptions::new().write(true)
                                                 .create(true)
                                                 .truncate(true)
                                                 .open("/.snapshots/dsnap/upstate")?;
                file.write_all("0 ".as_bytes())?;
                let mut file = OpenOptions::new().append(true)
                                                 .open("/.snapshots/dsnap/upstate")?;
                let date = Command::new("date").output()?;
                file.write_all(format!("\n{}", &date.stdout.to_string_lossy().as_str()?).as_bytes())?;
            } else {
                chr_delete(snapshot)?;
                let mut file = OpenOptions::new().write(true)
                                                 .create(true)
                                                 .truncate(true)
                                                 .open("/.snapshots/dsnap/upstate")?;
                file.write_all("1 ".as_bytes())?;
                let mut file = OpenOptions::new().append(true)
                                                 .open("/.snapshots/dsnap/upstate")?;
                let date = Command::new("date").output()?;
                file.write_all(format!("\n{}", &date.stdout.to_string_lossy().as_str()?).as_bytes())?;
                return Err(Error::new(ErrorKind::Other,
                                      "Failed to upgrade."));
            }
        }
    }
    Ok(())
}

// Reinstall base packages in snapshot
pub fn bootstrap(snapshot: &str) -> Result<(), Error> {
    // tmp database
    let tmp_db = TempDir::new_in("/.snapshots/tmp/")?;

    let dbpath = tmp_db.path().to_string_lossy();
    let target_path = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    let excode = Command::new("pacman")
        .arg(format!("--dbpath={}", dbpath))
        .arg("-r").arg(&target_path)
        .args(["-Sy", "--noconfirm", "--overwrite", "*", "base"])
        .status()?;
    if !excode.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to install base packages in snapshot {} chroot.", snapshot)));
    }

    let paru_args = format!("paru --dbpath {} -r {} -Sy --noconfirm --overwrite '*' distro-snapper",
                            dbpath, target_path);
    let paru_install = Command::new("su")
        .arg("aur")
        .arg("-c")
        .arg(paru_args)
        .status()?;
    if !paru_install.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to install distro-snapper package in snapshot {} chroot.", snapshot)));
    }

    // Copy pacman database from tmp
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/var/lib/pacman", snapshot))?;
    cp_reflink(format!("{}/.", tmp_db.path().to_string_lossy()),
               format!("/.snapshots/rootfs/snapshot-chr{}/var/lib/pacman", snapshot))?;
    Ok(())
}

// Copy cache of downloaded packages to shared
pub fn cache_copy(snapshot: &str, prepare: bool) -> Result<(), Error> {
    let tmp = get_tmp();
    if prepare {
        cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/var/cache/pacman/pkg/.", tmp),
                   format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/pacman/pkg", snapshot))?;
    } else {
        cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/pacman/pkg/.", snapshot),
                   format!("/.snapshots/rootfs/snapshot-{}/var/cache/pacman/pkg", tmp))?;
    }
    Ok(())
}

// Clean pacman cache
pub fn clean_cache(snapshot: &str) -> Result<(), Error> {
    if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/pacman/pkg", snapshot)).try_exists()? {
        remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/pacman/pkg", snapshot))?;
    }
    Ok(())
}

// Fix signature invalid error
pub fn fix_package_db(snapshot: &str) -> Result<(), Error> {
    // Make sure snapshot does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? && !snapshot.is_empty() {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot fix package man database as snapshot {} doesn't exist.", snapshot)));

        // Make sure snapshot is not in use
        } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(
            Error::new(ErrorKind::Unsupported,
                       format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                               snapshot,snapshot)));

    } else if snapshot.is_empty() && get_current_snapshot()? == "0" {
        // Base snapshot unsupported
        return Err(Error::new(ErrorKind::Unsupported, format!("Snapshot 0 (base) should not be modified.")));

    } else if snapshot == "0" {
        // Base snapshot unsupported
        return Err(Error::new(ErrorKind::Unsupported, format!("Snapshot 0 (base) should not be modified.")));

    } else {
        let run_chroot: bool;
        // If snapshot is current running
        run_chroot = if snapshot.is_empty() {
            false
        } else {
            true
        };

        // Snapshot is mutable so do not make it immutable after fixdb is done
        let flip = if check_mutability(snapshot) {
            false
        } else {
            if immutability_disable(snapshot).is_ok() {
                println!("Snapshot {} successfully made mutable.", snapshot);
            }
            true
        };

        // Fix package database
        if run_chroot {
            prepare(snapshot)?;
        }
        let mut cmds: Vec<Vec<String>> = Vec::new();
        // When run as real root (no sudo), fall back to root instead of panicking
        let username = std::env::var("SUDO_USER").unwrap_or_else(|_| "root".to_string());
        let user = get_user_by_name(&username).ok_or_else(|| Error::new(ErrorKind::Other,
                              "Failed to resolve user for package database repair."))?;
        let home_dir = user.home_dir();
        let home = home_dir.to_str().ok_or_else(|| Error::new(ErrorKind::Other,
                              "User home directory is not valid UTF-8."))?.to_string();
        if run_chroot {
            let chroot = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
            let etc_gnupg = format!("{}/etc/pacman.d/gnupg", chroot);
            if Path::new(&etc_gnupg).is_dir() && read_dir(&etc_gnupg)?.count() > 0 {
                std::fs::remove_dir_all(&etc_gnupg)?;
            }
            // In the chroot, run as root and use root's gnupg, not the invoking user's home
            let chroot_gnupg = format!("{}/root/.gnupg", chroot);
            if Path::new(&chroot_gnupg).is_dir() && read_dir(&chroot_gnupg)?.count() > 0 {
                std::fs::remove_dir_all(&chroot_gnupg)?;
            }
            let sync_dir = format!("{}/var/lib/pacman/sync", chroot);
            if Path::new(&sync_dir).is_dir() && read_dir(&sync_dir)?.count() > 0 {
                remove_dir_content(&sync_dir)?;
            }
            cmds.push(vec!["pacman".into(), "-Syy".into()]);
            cmds.push(vec!["gpg".into(), "--refresh-keys".into()]);
            cmds.push(vec!["pacman-key".into(), "--init".into()]);
            cmds.push(vec!["pacman-key".into(), "--populate".into(), "archlinux".into()]);
            cmds.push(vec!["pacman".into(), "-Syvv".into(), "--noconfirm".into(),
                           "archlinux-keyring".into()]);
        } else {
            if Path::new("/etc/pacman.d/gnupg").is_dir()
                && read_dir("/etc/pacman.d/gnupg")?.count() > 0 {
                std::fs::remove_dir_all("/etc/pacman.d/gnupg")?;
            }
            let user_gnupg = format!("{}/.gnupg", home);
            if Path::new(&user_gnupg).is_dir() && read_dir(&user_gnupg)?.count() > 0 {
                std::fs::remove_dir_all(&user_gnupg)?;
            }
            if Path::new("/var/lib/pacman/sync").is_dir()
                && read_dir("/var/lib/pacman/sync")?.count() > 0 {
                remove_dir_content("/var/lib/pacman/sync")?;
            }
            cmds.push(vec!["sudo".into(), "-u".into(), username.clone(),
                           "gpg".into(), "--refresh-keys".into()]);
            cmds.push(vec!["pacman-key".into(), "--init".into()]);
            cmds.push(vec!["pacman-key".into(), "--populate".into(), "archlinux".into()]);
        }
        for cmd in cmds {
            let mut command = if run_chroot {
                let mut command = Command::new("chroot");
                command.arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot));
                command
            } else {
                Command::new(&cmd[0])
            };
            if run_chroot {
                command.args(&cmd);
            } else {
                command.args(&cmd[1..]);
            }
            if !command.status()?.success() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Run command {} failed.", cmd.join(" "))));
            }
        }
        if snapshot.is_empty() {
            let snapshot = get_current_snapshot()?;
            prepare(&snapshot)?;
            refresh_helper(&snapshot)?;
        }

        // Return snapshot to immutable after fixdb is done if snapshot was immutable
        if flip {
            if immutability_enable(snapshot).is_ok() {
                println!("Snapshot {} successfully made immutable.", snapshot);
            }
        }
    }
    Ok(())
}

// Install atomic-operation
pub fn install_package_helper(snapshot:&str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    validate_package_specs(pkgs)?;
    let packages = pkgs.join(" ");
    audit_log("install-packages", &[("snapshot", snapshot), ("packages", packages.as_str())]);
    prepare(snapshot)?;
    //Profile configurations
    let cfile = format!("/.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot);
    let mut profconf = Ini::new_cs();
    profconf.set_comment_symbols(&['#']);
    profconf.set_multiline(true);
    let mut write_options = WriteOptions::default();
    write_options.blank_lines_between_sections = 1;
    // Load profile
    profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;

    for pkg in pkgs {
        let mut pkgs_list: Vec<String> = Vec::new();
        if let Some(section) = profconf.get_map().and_then(|map| map.get("profile-packages").cloned()) {
            for pkg in section.keys() {
                pkgs_list.push(pkg.to_string());
            }
        }
        // This extra pacman check is to avoid unwantedly triggering AUR if package is official
        let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
        let excode = Command::new("chroot").arg(&root)
                                           .args(["pacman", "-Si", pkg])
                                           .output()?; // --sysroot
        let excode_group = Command::new("chroot").arg(&root)
                                                 .args(["pacman", "-Sg", pkg])
                                                 .output()?;
        if excode.status.success() || excode_group.status.success() {
            let mut pacman_args = vec!["-S"];
            if noconfirm {
                pacman_args.push("--noconfirm");
            }
            pacman_args.extend_from_slice(&["--needed", "--overwrite", "/var/*"]);
            pacman_args.push(pkg.as_str());
            if run_chroot(&root, "pacman", &pacman_args).is_err() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to install {}", pkg)));
            // Add to profile-packages if not system package
            } else if !pkgs_list.contains(pkg) && !is_system_pkg(&profconf, pkg.to_string()) {
                pkgs_list.push(pkg.to_string());
                for key in pkgs_list {
                    profconf.remove_key("profile-packages", &key);
                    profconf.set("profile-packages", &key, None);
                }
                profile_write(&cfile, &profconf, &write_options)?;
            }
        } else if aur_check(snapshot)? {
            // Use paru if aur is enabled
            let paru_args = if noconfirm {
                format!("paru -S --noconfirm --needed --overwrite '/var/*' {}", pkg)
            } else {
                format!("paru -S --needed --overwrite '/var/*' {}", pkg)
            };
            if run_chroot(&root, "su", &["aur", "-c", &paru_args]).is_err() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to install {}", pkg)));
            // Add to profile-packages if not system package
            } else if !pkgs_list.contains(pkg) && !is_system_pkg(&profconf, pkg.to_string()) {
                pkgs_list.push(pkg.to_string());
                pkgs_list.sort();
                for key in pkgs_list {
                    profconf.remove_key("profile-packages", &key);
                    profconf.set("profile-packages", &key, None);
                }
                profile_write(&cfile, &profconf, &write_options)?;
            }
        } else if !aur_check(snapshot)? {
            return Err(Error::new(ErrorKind::NotFound,
                                  "Please enable AUR."));
        }
    }
    Ok(())
}

// Install atomic-operation
pub fn install_package_helper_chroot(snapshot:&str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    validate_package_specs(pkgs)?;
    let packages = pkgs.join(" ");
    audit_log("install-packages-chroot", &[("snapshot", snapshot), ("packages", packages.as_str())]);
    let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    for pkg in pkgs {
        // This extra pacman check is to avoid unwantedly triggering AUR if package is official
        let excode = Command::new("chroot").arg(&root)
                                           .args(["pacman", "-Si", pkg])
                                           .output()?; // --sysroot
        let excode_group = Command::new("chroot").arg(&root)
                                                 .args(["pacman", "-Sg", pkg])
                                                 .output()?;
        if excode.status.success() || excode_group.status.success() {
            let mut pacman_args = vec!["-Sy"];
            if noconfirm {
                pacman_args.push("--noconfirm");
            }
            pacman_args.extend_from_slice(&["--needed", "--overwrite", "*"]);
            pacman_args.push(pkg.as_str());
            if run_chroot(&root, "pacman", &pacman_args).is_err() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to install {}", pkg)));
            }
        } else if aur_check(snapshot)? {
            // Use paru if aur is enabled
            let paru_args = if noconfirm {
                format!("paru -Sy --noconfirm --needed --overwrite '*' {}", pkg)
            } else {
                format!("paru -Sy --needed --overwrite '*' {}", pkg)
            };
            if run_chroot(&root, "su", &["aur", "-c", &paru_args]).is_err() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to install {}", pkg)));
            }
        } else if !aur_check(snapshot)? {
            return Err(Error::new(ErrorKind::NotFound,
                                  "Please enable AUR."));
        }
    }
    Ok(())
}

// Install atomic-operation in live snapshot
pub fn install_package_helper_live(snapshot: &str, tmp: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    validate_package_specs(pkgs)?;
    let packages = pkgs.join(" ");
    audit_log("install-packages-live", &[("snapshot", tmp), ("packages", packages.as_str())]);
    for pkg in pkgs {
        // This extra pacman check is to avoid unwantedly triggering AUR
        // if package is official; probe inside the target snapshot so the
        // decision matches the chroot's package database
        let excode = Command::new("chroot")
                              .arg(format!("/.snapshots/rootfs/snapshot-{}", tmp))
                              .args(["pacman", "-Si", pkg])
                              .output()?;
        let excode_group = Command::new("chroot")
                                    .arg(format!("/.snapshots/rootfs/snapshot-{}", tmp))
                                    .args(["pacman", "-Sg", pkg])
                                    .output()?;
        if excode.status.success() || excode_group.status.success() {
            let mut pacman_args = vec!["-Sy"];
            if noconfirm {
                pacman_args.push("--noconfirm");
            }
            pacman_args.extend_from_slice(&["--overwrite", "*"]);
            pacman_args.push(pkg.as_str());
            let root = format!("/.snapshots/rootfs/snapshot-{}", tmp);
            if run_chroot(&root, "pacman", &pacman_args).is_err() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to install {}", pkg)));
            }
        } else if aur_check(snapshot)? {
            // Use paru if aur is enabled
            let paru_args = if noconfirm {
                format!("paru -Sy --noconfirm --overwrite '*' {}", pkg)
            } else {
                format!("paru -Sy --overwrite '*' {}", pkg)
            };
            let root = format!("/.snapshots/rootfs/snapshot-{}", tmp);
            if run_chroot(&root, "su", &["aur", "-c", &paru_args]).is_err() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to install {}", pkg)));
            }
        } else if !aur_check(snapshot)? {
            // Not an official package and AUR is not enabled: report the
            // same error as the non-live helper instead of silently
            // succeeding
            return Err(Error::new(ErrorKind::NotFound,
                                  "Please enable AUR."));
        }
    }
    Ok(())
}

// List persistent enabled systemd units in a snapshot or live root.
pub fn service_list_enabled(snapshot: &str, chr: &str) -> Result<Vec<String>, Error> {
    let target = format!("/.snapshots/rootfs/snapshot-{}{}", chr, snapshot);
    if !Path::new(&format!("{}/usr/lib/systemd/system", target)).try_exists()?
        && !Path::new(&format!("{}/etc/systemd/system", target)).try_exists()? {
        return Ok(Vec::new());
    }
    let output = Command::new("chroot").arg(&target)
                                      .args(["systemctl", "list-unit-files", "--state=enabled",
                                             "--no-legend", "--plain"])
                                      .output()?;
    if !output.status.success() {
        return Err(Error::new(ErrorKind::Other, "Failed to list enabled systemd units."));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines()
                .filter_map(|line| line.split_whitespace().next())
                .filter(|unit| !unit.is_empty())
                .map(|unit| unit.to_string())
                .collect())
}

// Prevent system packages from being automatically removed
pub fn lockpkg(snapshot:&str, profconf: &Ini) -> Result<(), Error> {
    let pacman_conf_path = format!("/.snapshots/rootfs/snapshot-chr{}/etc/pacman.conf", snapshot);

    // Collect the system packages the profile wants held
    let mut system_pkgs: Vec<String> = Vec::new();
    if let Some(section) = profconf.get_map().and_then(|map| map.get("system-packages").cloned()) {
        for pkg in section.keys() {
            system_pkgs.push(pkg.to_string());
        }
    }

    // Read the whole file
    let mut contents = String::new();
    let mut pfile = File::open(&pacman_conf_path)?;
    pfile.read_to_string(&mut contents)?;

    // Build the HoldPkg line
    let new_hold_pkgs = if !system_pkgs.is_empty() {
        let pkgs = system_pkgs.join(" ");
        format!("HoldPkg = {}", pkgs)
    } else {
        String::new()
    };

    // Replace the first existing HoldPkg line (commented or not);
    // otherwise append it. Never replace an empty string, that would
    // insert the value between every character of the file
    let mut replaced = false;
    let mut new_lines: Vec<String> = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if !replaced && (trimmed.starts_with("HoldPkg") || trimmed.starts_with("#HoldPkg")) {
            if !new_hold_pkgs.is_empty() {
                new_lines.push(new_hold_pkgs.clone());
            }
            replaced = true;
        } else {
            new_lines.push(line.to_string());
        }
    }
    if !replaced && !new_hold_pkgs.is_empty() {
        new_lines.push(new_hold_pkgs);
    }
    let mut new_contents = new_lines.join("\n");
    if contents.ends_with('\n') {
        new_contents.push('\n');
    }
    let mut nfile = File::create(&pacman_conf_path)?;
    nfile.write_all(new_contents.as_bytes())?;
    Ok(())
}

// Get list of installed packages and exclude packages installed as dependencies
pub fn no_dep_pkg_list(snapshot: &str, chr: &str) -> Result<Vec<String>, Error> {
    let root = format!("/.snapshots/rootfs/snapshot-{}{}", chr, snapshot);
    let excode = Command::new("chroot").arg(&root)
                                       .args(["pacman", "-Qqe"])
                                       .output()?;
    if !excode.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to list packages: {}",
                                      String::from_utf8_lossy(&excode.stderr).trim())));
    }
    let stdout = String::from_utf8_lossy(&excode.stdout).trim().to_string();
    Ok(stdout.split('\n').map(|s| s.to_string()).collect())
}

// Get list of packages installed in a snapshot
pub fn pkg_list(snapshot: &str, chr: &str) -> Result<Vec<String>, Error> {
    prepare(snapshot)?;
    let root = format!("/.snapshots/rootfs/snapshot-{}{}", chr, snapshot);
    let output = Command::new("chroot").arg(&root)
                                       .args(["pacman", "-Qq"])
                                       .output();
    // Always tear down the chroot, even when the query itself failed.
    let cleanup = post_transactions(snapshot);
    let excode = output?;
    cleanup?;
    if !excode.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to list packages: {}",
                                      String::from_utf8_lossy(&excode.stderr).trim())));
    }
    let stdout = String::from_utf8_lossy(&excode.stdout).trim().to_string();
    Ok(stdout.split('\n').map(|s| s.to_string()).collect())
}

// Pacman query
pub fn pkg_query(pkg: &str) -> Result<String, Error> {
    let output = Command::new("pacman")
                      .args(["-Q", pkg])
                      .output()?;
    if !output.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Package {} not found.", pkg)));
    }
    // pacman -Q prints "pkg version"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.split_whitespace().nth(1)
                        .ok_or_else(|| Error::new(ErrorKind::Other,
                                                   format!("Failed to parse version for package {}.", pkg)))?;
    Ok(version.to_string())
}

// Refresh snapshot atomic-operation
pub fn refresh_helper(snapshot: &str) -> Result<(), Error> {
    let refresh = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                        .args(["pacman", "-Syy"])
                                        .status()?;
    // Avoid invalid or corrupted package (PGP signature) error
    let keyring = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                        .args(["pacman", "-S", "--noconfirm", "archlinux-keyring"])
                                        .status()?;
    if !refresh.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Refresh failed."));
    }
    if !keyring.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to update archlinux-keyring."));
    }
   Ok(())
}

// Disable service(s)
pub fn service_disable(snapshot: &str, services: &Vec<String>, chr: &str) -> Result<(), Error> {
    validate_service_names(services)?;
    let names = services.join(" ");
    audit_log("service-disable", &[("snapshot", snapshot), ("services", names.as_str())]);
    for service in services {
        // Systemd
        if has_systemd(snapshot, chr)? {
            let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-{}{}", chr,snapshot))
                                               .args(["systemctl", "disable", "--"])
                                               .arg(&service).status()?;
            if !excode.success() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to disable {}.", service)));
            }
        } //TODO add OpenRC
    }
    Ok(())
}

// Enable service(s)
pub fn service_enable(snapshot: &str, services: &Vec<String>, chr: &str) -> Result<(), Error> {
    validate_service_names(services)?;
    let names = services.join(" ");
    audit_log("service-enable", &[("snapshot", snapshot), ("services", names.as_str())]);
    for service in services {
        // Systemd
        if has_systemd(snapshot, chr)? {
            let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-{}{}", chr,snapshot))
                                               .args(["systemctl", "enable", "--"])
                                               .arg(&service).status()?;
            if !excode.success() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to enable {}.", service)));
            }
        } //TODO add OpenRC
    }
    Ok(())
}

// Copy system configurations to new snapshot
pub fn system_config(snapshot: &str, profconf: &Ini) -> Result<(), Error> {
    // Copy [fstab, time ,localization, network configuration, users and groups, pacman.conf]
    let files = vec!["/etc/fstab", "/etc/localtime", "/etc/adjtime", "/etc/locale.gen", "/etc/locale.conf",
                     "/etc/vconsole.conf", "/etc/hostname", "/etc/shadow", "/etc/passwd", "/etc/gshadow",
                     "/etc/group", "/etc/sudoers", "/etc/pacman.conf"];

    for file in files {
        if Path::new(&format!("/.snapshots/rootfs/snapshot-{}{}", snapshot,file)).is_file() {
            cp_reflink(format!("/.snapshots/rootfs/snapshot-{}{}", snapshot,file),
                       format!("/.snapshots/rootfs/snapshot-chr{}{}", snapshot,file))?;
        }
    }

    // Copy pacman.d directory
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/etc/pacman.d", snapshot))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/etc/pacman.d/.", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/etc/pacman.d/", snapshot))?;

    // Copy dsnap configuration
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/etc/dsnap", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/etc/dsnap", snapshot))?;

    // Copy grub configuration
    #[cfg(feature = "grub")]
    let grub = grub::get_grub(snapshot)?;
    #[cfg(feature = "grub")]
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/boot/{}/.", snapshot,grub),
               format!("/.snapshots/rootfs/snapshot-chr{}/boot/{}", snapshot,grub))?;
    #[cfg(feature = "grub")]
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/etc/default/grub", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/etc/default/grub", snapshot))?;
    #[cfg(feature = "grub")]
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/etc/grub.d", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/etc/grub.d", snapshot))?;

    // Copy /usr/local
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/usr/local", snapshot))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/usr/local/.", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/usr/local/", snapshot))?;

    // Install system packages
    if let Some(section) = profconf.get_map().and_then(|map| map.get("system-packages").cloned()) {
        let mut pkgs_list: Vec<String> = Vec::new();
        for pkg in section.keys() {
            pkgs_list.push(pkg.to_string());
        }
        if !pkgs_list.is_empty() {
            install_package_helper_chroot(snapshot, &pkgs_list,true)?;
        }
    }

    if let Some(section) = profconf.get_map().and_then(|map| map.get("profile-packages").cloned()) {
        let mut pkgs_list: Vec<String> = Vec::new();
        for pkg in section.keys() {
            pkgs_list.push(pkg.to_string());
        }
        if !pkgs_list.is_empty() {
            install_package_helper_chroot(snapshot, &pkgs_list,true)?;
        }
    }

    // Read enable services section in configuration file
    if let Some(section) = profconf.get_map().and_then(|map| map.get("enable-services").cloned()) {
        let mut services: Vec<String> = Vec::new();
        for service in section.keys() {
            services.push(service.to_string());
        }
        // Enable service(s)
        if !services.is_empty() {
            service_enable(snapshot, &services, "chr")?;
        }
    }

    // Read commands section in configuration file
    if let Some(section) = profconf.get_map().and_then(|map| map.get("install-commands").cloned()) {
        for cmd in section.keys() {
            chroot_exec(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot), cmd)?;
        }
    }

    // Restore system configuration
    if let Some(section) = profconf.get_map().and_then(|map| map.get("system-configuration").cloned()) {
        let mut system_conf: Vec<String> = Vec::new();
        for path in section.keys() {
            validate_system_config_path(path)?;
            // Check if a file or directory exists
            if !metadata(path).is_ok() {
                system_conf.push(path.to_string());
            }
        }
        if !system_conf.is_empty() {
            for path in system_conf {
                cp_reflink(format!("/.snapshots/rootfs/snapshot-{}{}", snapshot,path),
                           format!("/.snapshots/rootfs/snapshot-chr{}{}", snapshot,path))?;
            }
        }
    }

    Ok(())
}

// Sync tree helper function
pub fn tree_sync_helper(s_f: &str, s_t: &str, chr: &str) -> Result<(), Error>  {
    cp_reflink(format!("/.snapshots/etc/etc-{}/.", s_f),
               format!("/.snapshots/rootfs/snapshot-{}{}/etc", chr,s_t))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/var/cache/pacman/pkg/.", s_f),
               format!("/.snapshots/rootfs/snapshot-{}{}/var/cache/pacman/pkg", chr,s_t))?;
    // Profile reconciliation only applies to the chroot flow; the live
    // post-sync targets the deployed slot, where neither profile path of
    // check_profile exists (snapshot-chr{tmp} is never created there)
    if !chr.is_empty() {
        check_profile(s_t)?;
    }
    Ok(())
}

// Uninstall package(s) atomic-operation
pub fn uninstall_package_helper(snapshot: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    validate_package_specs(pkgs)?;
    let packages = pkgs.join(" ");
    audit_log("uninstall-packages", &[("snapshot", snapshot), ("packages", packages.as_str())]);
    // Profile configurations
    let cfile = format!("/.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot);
    let mut profconf = Ini::new_cs();
    profconf.set_comment_symbols(&['#']);
    profconf.set_multiline(true);
    let mut write_options = WriteOptions::default();
    write_options.blank_lines_between_sections = 1;
    // Load profile
    profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;

    for pkg in pkgs {
        let mut pkgs_list: Vec<String> = Vec::new();
        if let Some(section) = profconf.get_map().and_then(|map| map.get("profile-packages").cloned()) {
            for pkg in section.keys() {
                pkgs_list.push(pkg.to_string());
            }
        }
        let pacman_args: &[&str] = if noconfirm {
            &["pacman", "--noconfirm", "-Rns"]
        } else {
            &["pacman", "-Rns"]
        };

        if !is_system_locked() || !is_system_pkg(&profconf, pkg.to_string()) {
            let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                               .args(pacman_args)
                                               .arg(format!("{}", pkg)).status()?;

            if !excode.success() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to uninstall {}", pkg)));
            } else if pkgs_list.contains(pkg) {
                profconf.remove_key("profile-packages", &pkg);
                profile_write(&cfile, &profconf, &write_options)?;
            } else if is_system_pkg(&profconf, pkg.to_string()) {
                profconf.remove_key("system-packages", &pkg);
                profile_write(&cfile, &profconf, &write_options)?;
            }
        } else if is_system_locked() && is_system_pkg(&profconf, pkg.to_string()){
            return Err(Error::new(ErrorKind::Unsupported,
                                  "Remove system package(s) is not allowed."));
        }
    }
    Ok(())
}

// Uninstall package(s) atomic-operation
pub fn uninstall_package_helper_chroot(snapshot: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    validate_package_specs(pkgs)?;
    let packages = pkgs.join(" ");
    audit_log("uninstall-packages-chroot", &[("snapshot", snapshot), ("packages", packages.as_str())]);
    for pkg in pkgs {
        let pacman_args: &[&str] = if noconfirm {
            &["pacman", "--noconfirm", "-Rns"]
        } else {
            &["pacman", "-Rns"]
        };

        let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                           .args(pacman_args)
                                           .arg(format!("{}", pkg)).status()?;

        if !excode.success() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to uninstall {}", pkg)));
        }
    }
    Ok(())
}

// Uninstall package(s) atomic-operation live snapshot
pub fn uninstall_package_helper_live(tmp: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    validate_package_specs(pkgs)?;
    let packages = pkgs.join(" ");
    audit_log("uninstall-packages-live", &[("snapshot", tmp), ("packages", packages.as_str())]);
    for pkg in pkgs {
        let pacman_args: &[&str] = if noconfirm {
            &["pacman", "--noconfirm", "-Rns"]
        } else {
            &["pacman", "-Rns"]
        };

        let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-{}", tmp))
                                           .args(pacman_args)
                                           .arg(format!("{}", pkg)).status()?;

        if !excode.success() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to uninstall {}", pkg)));
        }
    }
    Ok(())
}

// Upgrade snapshot atomic-operation
pub fn upgrade_helper(snapshot: &str, noconfirm: bool) -> Result<(), Error> {
    // Prepare snapshot
    prepare(snapshot)?;
    // Avoid invalid or corrupted package (PGP signature) error
    let pacman_args: &[&str] = if noconfirm {
        &["pacman", "--noconfirm", "-Syy", "archlinux-keyring"]
    } else {
        &["pacman", "-Syy", "archlinux-keyring"]
    };

    let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                       .args(pacman_args)
                                       .status()?;
    if !excode.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to refresh archlinux-keyring."));
    }
    if !aur_check(snapshot)? {
        let pacman_args: &[&str] = if noconfirm {
            &["pacman", "--noconfirm", "-Syyu"]
        } else {
            &["pacman", "-Syyu"]
        };

        let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                            .args(pacman_args)
                                            .status()?;
        if !excode.success() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to upgrade snapshot {}.", snapshot)));
        }
    } else {
        let paru_args = if noconfirm {
            "paru --noconfirm -Syyu"
        } else {
            "paru -Syyu"
        };

        let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
        if run_chroot(&root, "su", &["aur", "-c", paru_args]).is_err() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to upgrade snapshot {}.", snapshot)));
        }
    }
    Ok(())
}

// Live upgrade snapshot atomic-operation
pub fn upgrade_helper_live(tmp: &str, noconfirm: bool) -> Result<(), Error> {
    // Avoid invalid or corrupted package (PGP signature) error
    let pacman_args: &[&str] = if noconfirm {
        &["pacman", "--noconfirm", "-Syy", "archlinux-keyring"]
    } else {
        &["pacman", "-Syy", "archlinux-keyring"]
    };

    let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-{}", tmp))
                                       .args(pacman_args)
                                       .status()?;
    if !excode.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to refresh archlinux-keyring for current/live snapshot."));
    }
    if !aur_check(tmp)? {
        let pacman_args: &[&str] = if noconfirm {
            &["pacman", "--noconfirm", "-Syyu"]
        } else {
            &["pacman", "-Syyu"]
        };

        let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-{}", tmp))
                                           .args(pacman_args)
                                           .status()?;
        if !excode.success() {
            return Err(Error::new(ErrorKind::Other,
                                  "Failed to upgrade current/live snapshot."));
        }
    } else {
        let paru_args = if noconfirm {
            "paru --noconfirm -Syyu"
        } else {
            "paru -Syyu"
        };

        let root = format!("/.snapshots/rootfs/snapshot-{}", tmp);
        if run_chroot(&root, "su", &["aur", "-c", paru_args]).is_err() {
            return Err(Error::new(ErrorKind::Other,
                                  "Failed to upgrade current/live snapshot."));
        }
    }
    Ok(())
}
