mod dsnapkg;
pub mod detect_distro;
mod tree;

use chrono::Local;
use configparser::ini::{Ini, WriteOptions};
use ctrlc;
use curl::easy::{Easy, HttpVersion, List};
use detect_distro as detect;
use nix::mount::{MntFlags, mount, MsFlags, umount2};
use partition_identity::{PartitionID, PartitionSource};
use proc_mounts::MountIter;
use rustix::path::Arg;
use std::collections::HashMap;
use std::fs::{copy, DirBuilder, File, OpenOptions, read_dir, read_to_string};
use std::io::{BufRead, BufReader, Error, ErrorKind, Read, stdin, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use tree::*; // TODO Drop python-anytree
use walkdir::WalkDir;

//  Select file system foramt
// Default use btrfs
#[cfg(feature = "btrfs")]
mod btrfs;
#[cfg(feature = "btrfs")]
use btrfs::*;
//#[cfg(feature = "zfs")] //TODO
//mod zfs;
//#[cfg(feature = "zfs")] //TODO
//use zfs::*;

// Select bootloader
#[cfg(feature = "grub")]
pub mod grub;
#[cfg(feature = "grub")]
use grub::*;
//TODO add systemd-boot

// Select package manager
//#[cfg(feature = "apk")]
//use dsnapkg::apk::*; // TODO
#[cfg(feature = "apt")]
use dsnapkg::apt::*;
#[cfg(feature = "dnf")]
use dsnapkg::dnf::*;
//#[cfg(feature = "pkgtool")] // TODO
//use dsnapkg::pkgtool::*;
//#[cfg(feature = "portage")] // TODO
//use dsnapkg::portage::*;
//#[cfg(feature = "xbps")] // TODO
//use dsnapkg::xbps::*;
#[cfg(feature = "pacman")]
// Default
use dsnapkg::pacman::*;

// Check if directory mutable
fn allow_dir_mut(mount_path: &str) -> bool {
    let mount_str = format!("/{}", mount_path);
    let path = Path::new(&mount_str);
    let not_allowed = vec!["bin", "dev", "lib", "lib64", "proc", "sbin", "usr", "usr/bin", "usr/lib", "usr/lib64", "usr/sbin"];
    if path.is_dir() && !not_allowed.contains(&mount_path.trim_end_matches("/")) && !mount_path.starts_with("/") {
        return true;
    } else {
        return false;
    }
}

// dsnap chroot mounts
pub fn dsnap_mounts(i: &str, chr: &str) -> nix::Result<()> {
    #[cfg(feature = "btrfs")]
    let file_system = "btrfs";
    let snapshot_path = format!("/.snapshots/rootfs/snapshot-{}{}", chr, i);

    // Mount snapshot to itself as a bind mount
    mount(Some(snapshot_path.as_str()), snapshot_path.as_str(),
          Some(file_system), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    // Mount /dev
    mount(Some("/dev"), format!("{}/dev", snapshot_path).as_str(),
          Some(file_system), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    // Mount /etc
    if chr != "chr" {
        mount(Some("/etc"), format!("{}/etc", snapshot_path).as_str(),
              Some(file_system), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    }
    // Mount /home
    mount(Some("/home"), format!("{}/home", snapshot_path).as_str(),
          Some(file_system), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    // Mount /proc
    mount(Some("/proc"), format!("{}/proc", snapshot_path).as_str(),
          Some("proc"), MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV, None::<&str>)?;
    // Mount /root
    mount(Some("/root"), format!("{}/root", snapshot_path).as_str(),
          Some(file_system), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    // Mount /run
    mount(Some("/run"), format!("{}/run", snapshot_path).as_str(),
          Some("tmpfs"), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    // Mount /sys
    mount(Some("/sys"), format!("{}/sys", snapshot_path).as_str(),
          Some("sysfs"), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    // Mount /tmp
    mount(Some("/tmp"), format!("{}/tmp", snapshot_path).as_str(),
          Some("tmpfs"), MsFlags::MS_BIND | MsFlags::MS_REC | MsFlags::MS_SLAVE, None::<&str>)?;
    // Mount /var
    if chr != "chr" {
    mount(Some("/var"), format!("{}/var", snapshot_path).as_str(),
          Some(file_system), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    }

    // Check EFI
    if is_efi() {
        // Mount /sys/firmware/efi/efivars
        mount(Some("/sys/firmware/efi/efivars"), format!("{}/sys/firmware/efi/efivars", snapshot_path).as_str(),
              Some("efivarfs"), MsFlags::MS_BIND | MsFlags::MS_REC | MsFlags::MS_SLAVE, None::<&str>)?;
    }

    // Mount /etc/resolv.conf
    if Path::new(&format!("{}/etc/resolv.conf", snapshot_path)).try_exists().map_err(|e| nix::errno::Errno::from_i32(e.raw_os_error().unwrap_or(5)))? {
        mount(Some("/etc/resolv.conf"), format!("{}/etc/resolv.conf", snapshot_path).as_str(),
              Some(file_system), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    } else {
        // Fedora links file to /run/systemd/resolve/stub-resolv.conf
        DirBuilder::new().recursive(true)
                         .create(format!("{}/run/systemd/resolve", snapshot_path)).map_err(|e| nix::errno::Errno::from_i32(e.raw_os_error().unwrap_or(5)))?;
        File::create(format!("{}/run/systemd/resolve/stub-resolv.conf", snapshot_path)).map_err(|e| nix::errno::Errno::from_i32(e.raw_os_error().unwrap_or(5)))?;
        mount(Some("/etc/resolv.conf"), format!("{}/run/systemd/resolve/stub-resolv.conf", snapshot_path).as_str(),
              Some(file_system), MsFlags::MS_BIND | MsFlags::MS_SLAVE, None::<&str>)?;
    }

    Ok(())
}

// dsnap chroot umounts
pub fn dsnap_umounts(i: &str, chr: &str) -> nix::Result<()> {
    let snapshot_path = format!("/.snapshots/rootfs/snapshot-{}{}", chr, i);

    // Umount in reverse order
    // Umount /etc/resolv.conf
    if Path::new(&format!("{}/etc/resolv.conf", snapshot_path)).try_exists().map_err(|e| nix::errno::Errno::from_i32(e.raw_os_error().unwrap_or(5)))? {
        umount2(Path::new(&format!("{}/etc/resolv.conf", snapshot_path)),
                MntFlags::empty())?;
    } else {
        umount2(Path::new(&format!("{}/run/systemd/resolve/stub-resolv.conf", snapshot_path)),
                MntFlags::empty())?;
        // Only remove the file we created; /run is a bind of the host's
        // /run (or a shared path), so removing the directory would
        // destroy the live host's systemd-resolved state
        let _ = std::fs::remove_file(format!("{}/run/systemd/resolve/stub-resolv.conf", snapshot_path));
    }

    // Check EFI
    if is_efi() {
        // Umount /sys/firmware/efi/efivars
        umount2(Path::new(&format!("{}/sys/firmware/efi/efivars", snapshot_path)),
                MntFlags::empty())?;
    }

    // Umount /var
    if chr != "chr" {
        umount2(Path::new(&format!("{}/var", snapshot_path)),
                MntFlags::empty())?;
    }
    // Umount chroot /tmp
    umount2(Path::new(&format!("{}/tmp", snapshot_path)),
            MntFlags::MNT_DETACH)?;
    // Umount chroot /sys
    umount2(Path::new(&format!("{}/sys", snapshot_path)),
            MntFlags::MNT_DETACH)?;
    // Umount chroot /run
    umount2(Path::new(&format!("{}/run", snapshot_path)),
            MntFlags::MNT_DETACH)?;
    // Umount chroot /root
    umount2(Path::new(&format!("{}/root", snapshot_path)),
            MntFlags::MNT_DETACH)?;
    // Umount chroot /proc
    umount2(Path::new(&format!("{}/proc", snapshot_path)),
            MntFlags::MNT_DETACH)?;
    // Umount chroot /home
    umount2(Path::new(&format!("{}/home", snapshot_path)),
            MntFlags::MNT_DETACH)?;
    // Umount chroot /etc
    if chr != "chr" {
        umount2(Path::new(&format!("{}/etc", snapshot_path)),
                MntFlags::MNT_DETACH)?;
    }
    // Umount chroot /dev
    umount2(Path::new(&format!("{}/dev", snapshot_path)),
            MntFlags::MNT_DETACH)?;
    // Umount chroot directory
    umount2(Path::new(&format!("{}", snapshot_path)),
            MntFlags::MNT_DETACH)?;

    Ok(())
}

//dsnap version
pub fn dsnap_version() -> Result<String, Error> {
    let pkg = "distro-snapper";
    let version = pkg_query(pkg)?;
    Ok(version)
}

// Add node to branch
pub fn branch_create(snapshot: &str, desc: &str) -> Result<i32, Error> {
    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot branch as snapshot {} doesn't exist.", snapshot)));

    } else {
        // Find the next available snapshot number
        let i = find_new()?;
        // Check mutability
        let immutability = if check_mutability(snapshot) {
            false
        } else {
            true
        };

        // Create snapshot
        create_snapshot(&format!("/.snapshots/boot/boot-{}", snapshot),
                        &format!("/.snapshots/boot/boot-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/etc/etc-{}", snapshot),
                        &format!("/.snapshots/etc/etc-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/var/var-{}", snapshot),
                        &format!("/.snapshots/var/var-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-{}", snapshot),
                        &format!("/.snapshots/rootfs/snapshot-{}", i),
                        immutability)?;

        // Mark newly created snapshot as mutable
        if !immutability {
            File::create(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/mutable", i))?;
        }

        // Import tree file
        let tree = fstree()?;
        // Add child to node
        add_node_to_parent(&tree, snapshot, i)?;
        // Save tree to fstree
        write_tree(&tree)?;
        // Write description for snapshot
        if !desc.is_empty() {
            write_desc(&i.to_string(), &desc, true)?;
        }
        Ok(i)
    }
}

// Check if snapshot is mutable
pub fn check_mutability(snapshot: &str) -> bool {
    Path::new(&format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/mutable", snapshot))
        .try_exists()
        .unwrap_or(false)
}

// Check whether a snapshot or live root uses systemd. Detection is done
// inside the target, never on the host running dsnap.
pub fn has_systemd(snapshot: &str, chr: &str) -> Result<bool, Error> {
    let root = format!("/.snapshots/rootfs/snapshot-{}{}", chr, snapshot);
    Ok(Path::new(&format!("{}/usr/lib/systemd/system", root)).try_exists()?
       || Path::new(&format!("{}/etc/systemd/system", root)).try_exists()?)
}

// Check if snapshot profile was changed
fn check_profile(snapshot: &str) -> Result<(), Error> {
    println!("Saving the changes, Please wait.");
    let mut write_options = WriteOptions::default();
    write_options.blank_lines_between_sections = 1;

    fn section_keys(conf: &Ini, section: &str) -> Vec<String> {
        conf.get_map()
            .and_then(|m| m.get(section).cloned())
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }

    // Get values before edit
    let old_cfile = format!("/.snapshots/etc/etc-{}/dsnap/profile", snapshot);
    let mut old_profconf = Ini::new_cs();
    old_profconf.set_comment_symbols(&['#']);
    old_profconf.set_multiline(true);
    old_profconf.load(&old_cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let old_system_pkgs = section_keys(&old_profconf, "system-packages");
    let old_pkgs = section_keys(&old_profconf, "profile-packages");
    let old_enable = section_keys(&old_profconf, "enable-services");

    // Get new values
    let cfile = format!("/.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot);
    let mut profconf = Ini::new_cs();
    profconf.set_comment_symbols(&['#']);
    profconf.set_multiline(true);
    profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let new_system_pkgs = section_keys(&profconf, "system-packages");
    let new_pkgs = section_keys(&profconf, "profile-packages");
    let new_enable = section_keys(&profconf, "enable-services");

    // Apply changes
    // Install new system package(s)
    let mut system_pkgs_to_install: Vec<String> = Vec::new();
    for pkg in &new_system_pkgs {
        if !old_system_pkgs.contains(&pkg) {
            system_pkgs_to_install.push(pkg.to_string());
        }
    }
    if !system_pkgs_to_install.is_empty() && !is_system_locked() {
        install_package_helper_chroot(snapshot, &system_pkgs_to_install, true)?;
    } else if !system_pkgs_to_install.is_empty() && is_system_locked() {
        // Prevent install new system package(s)
        return Err(Error::new(ErrorKind::Unsupported, format!("Install system package(s) is not allowed.")));
    }

    // Install new profile package(s)
    let mut pkgs_to_install: Vec<String> = Vec::new();
    for pkg in &new_pkgs {
        if !old_pkgs.contains(&pkg) {
            pkgs_to_install.push(pkg.to_string());
        }
    }
    if !pkgs_to_install.is_empty() {
        install_package_helper_chroot(snapshot, &pkgs_to_install, true)?;
    }

    // Disable service(s) removed from enable-services
    let mut services_to_disable: Vec<String> = Vec::new();
    for service in &old_enable {
        if !new_enable.contains(service) {
            services_to_disable.push(service.to_string());
        }
    }
    if !services_to_disable.is_empty() {
        service_disable(snapshot, &services_to_disable, "chr")?;
    }

    // Enable new service(s)
    let mut services_to_enable: Vec<String> = Vec::new();
    for service in &new_enable {
        if !old_enable.contains(service) {
            services_to_enable.push(service.to_string());
        }
    }
    if !services_to_enable.is_empty() {
        service_enable(snapshot, &services_to_enable, "chr")?;
    }

    // Uninstall package(s) not in the new system-packages list
    let mut system_pkgs_to_uninstall: Vec<String> = Vec::new();
    for pkg in &old_system_pkgs {
        if !new_system_pkgs.contains(pkg) && !new_pkgs.contains(pkg) {
            system_pkgs_to_uninstall.push(pkg.to_string());
        }
    }
    if !system_pkgs_to_uninstall.is_empty() && !is_system_locked() {
        uninstall_package_helper_chroot(snapshot, &system_pkgs_to_uninstall, true)?;
    } else if !system_pkgs_to_uninstall.is_empty() && is_system_locked() {
        // Prevent remove of system package from profile
        return Err(Error::new(ErrorKind::Unsupported, "Remove system package(s) is not allowed.".to_string()));
    }

    // Uninstall package(s) not in the new profile-packages list
    let mut pkgs_to_uninstall: Vec<String> = Vec::new();
    for pkg in &old_pkgs {
        if !new_pkgs.contains(pkg) && !new_system_pkgs.contains(pkg) {
            pkgs_to_uninstall.push(pkg.to_string());
        }
    }
    if !pkgs_to_uninstall.is_empty() {
        uninstall_package_helper_chroot(snapshot, &pkgs_to_uninstall, true)?;
    }

    // Check database
    let pkg_list = no_dep_pkg_list(snapshot, "chr")?;
    for pkg in &pkg_list {
        let mut pkgs: Vec<String> = Vec::new();
        if profconf.sections().contains(&"system-packages".to_string()) {
            for sys_pkg in section_keys(&profconf, "system-packages") {
                if !pkg_list.contains(&sys_pkg) && !is_system_locked() {
                    profconf.remove_key("system-packages", &sys_pkg);
                } else if !pkg_list.contains(&sys_pkg) && is_system_locked() {
                    return Err(Error::new(ErrorKind::Unsupported, "Remove system package(s) is not allowed.".to_string()));
                }
            }
            profile_write(&cfile, &profconf, &write_options)?;
        }
        if profconf.sections().contains(&"profile-packages".to_string()) {
            for prof_pkg in section_keys(&profconf, "profile-packages") {
                if !pkg_list.contains(&prof_pkg) {
                    profconf.remove_key("profile-packages", &prof_pkg);
                } else {
                    pkgs.push(prof_pkg);
                }
            }
            profile_write(&cfile, &profconf, &write_options)?;
        }
        if !pkgs.contains(pkg) {
            profconf.set("profile-packages", pkg, None);
        }
        profile_write(&cfile, &profconf, &write_options)?;
    }

    // Keep enable-services in sync with the units actually enabled in the
    // snapshot, the same way profile-packages tracks installed packages.
    let enabled_units = service_list_enabled(snapshot, "chr")?;
    for unit in section_keys(&profconf, "enable-services") {
        if !enabled_units.contains(&unit) {
            profconf.remove_key("enable-services", &unit);
        }
    }
    for unit in &enabled_units {
        profconf.set("enable-services", unit, None);
    }
    profile_write(&cfile, &profconf, &write_options)?;

    // Prevent duplication
    let current_profile_pkgs = section_keys(&profconf, "profile-packages");
    for pkg in &new_system_pkgs {
        if current_profile_pkgs.contains(pkg) {
            profconf.remove_key("profile-packages", pkg);
        }
    }
    profile_write(&cfile, &profconf, &write_options)?;

    // Block automatic removal of system packages
    lockpkg(snapshot, &profconf)?;

    Ok(())
}

// Check if last update was successful
pub fn check_update() -> Result<(), Error> {
    // Open and read upstate file
    let upstate = File::open("/.snapshots/dsnap/upstate")?;
    let buf_read = BufReader::new(upstate);
    let mut read = buf_read.lines();

    // Read state line
    let line = read.next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Update state file is missing state."))??;
    // Read data line
    let data = read.next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Update state file is missing date."))??;

    // Check state line
    if line.contains("1") {
        eprintln!("Last update on {} failed.", data);
    } else if line.contains("0") {
        println!("Last update on {} completed successfully.", data);
    }
    Ok(())
}

// Clean chroot mount directories for a snapshot
pub fn chr_delete(snapshot: &str) -> Result<(), Error> {
    // Path to mount directories
    let chr_path = vec!["boot", "etc", "var"];
    for path in chr_path {
        if Path::new(&format!("/.snapshots/{}/{}-chr{}", path,path,snapshot)).try_exists()? {
            // Copy cache back to tmp
            if path == "var" {
                cache_copy(snapshot, false)?;
            }
            // Delete boot,etc and var subvolumes
            delete_subvolume(&format!("/.snapshots/{}/{}-chr{}", path,path,snapshot))?;
        }
    }

    // Path to snapshot mount directory
    let snapshot_path = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);

    // Delete snapshot subvolume
    if Path::new(&snapshot_path).try_exists()? {
        // Try to remove directory contents if delete subvolume failed
        if delete_subvolume(&snapshot_path).is_err() {
            if path_is_mounted_or_under(Path::new(&snapshot_path))? {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Cannot remove {} because it is still mounted.", snapshot_path)));
            }
            remove_dir_content(&snapshot_path)?;
            delete_subvolume(&snapshot_path)?;
        }
    }
    Ok(())
}

// Run command in snapshot
pub fn chroot(snapshot: &str, cmds: Vec<String>) -> Result<(), Error> {
    let path = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);

    // Make sure snapshot does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot chroot as snapshot {} doesn't exist.", snapshot)));

    } else if Path::new(&path).try_exists()? {
        // Make sure snapshot is not in use by another dsnap process
        return Err(Error::new
                   (ErrorKind::Unsupported,
                    format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                            snapshot,snapshot)));

    } else if snapshot == "0" {
        // Make sure snapshot is not base snapshot
        return Err(Error::new(ErrorKind::Unsupported, format!("Changing base snapshot is not allowed.")));

    } else {
        // Prepare snapshot for chroot and run command if existed
        if !cmds.is_empty() {
            // Chroot to snapshot path
            for cmd in cmds {
                prepare(snapshot)?;
                // Run command in chroot
                if let Err(e) = chroot_exec(&path, &cmd) {
                    chr_delete(snapshot)?;
                    return Err(e);
                }
                // Check for profile changes
                if check_profile(snapshot).is_err() {
                    chr_delete(snapshot)?;
                    return Err(Error::new(ErrorKind::Other,
                                          "Profile check failed and changes discarded.".to_string()));
                }
                // Make sure post_transactions exit properly
                if let Err(error) = post_transactions(snapshot) {
                    let _ = chr_delete(snapshot);
                    return Err(Error::new(ErrorKind::Other,
                                          format!("post_transactions error: {}", error)));
                }
            }
        } else {
            prepare(snapshot)?;
            // Chroot
            let status = chroot_in(&path)?;
            if status.code().is_none() {
                chr_delete(snapshot)?;
                return Err(Error::new(ErrorKind::Interrupted,
                                      "Chroot process terminated by signal.".to_string()));
            }
            // Check for profile changes
            if check_profile(snapshot).is_err() {
                chr_delete(snapshot)?;
                return Err(Error::new(ErrorKind::Other,
                                      "Profile check failed and changes discarded.".to_string()));
            }
            // Make sure post_transactions exit properly
            if let Err(error) = post_transactions(snapshot) {
                let _ = chr_delete(snapshot);
                return Err(Error::new(ErrorKind::Other,
                                      format!("post_transactions error: {}", error)));
            }
        }
    }
    Ok(())
}

// Check if inside chroot
pub fn chroot_check() -> Result<bool, Error> {
    let read = read_to_string("/proc/mounts")?;
    #[cfg(feature = "btrfs")]
    {
        Ok(!read.contains("/.snapshots btrfs"))
    }
    #[cfg(not(feature = "btrfs"))]
    {
        let _ = read;
        Ok(false)
    }
}

// Run command in chroot
pub fn chroot_exec(path: &str,cmd: &str) -> Result<(), Error> {
    let exocde = Command::new("sh").arg("-c").arg(format!("chroot {} {}", path,cmd)).status()?;
    if !exocde.success() {
        return Err(
            Error::new(
                ErrorKind::Other,
                format!("Failed to run {}.", cmd)));
    }
    Ok(())
}

// Enter chroot
pub fn chroot_in(path: &str) -> Result<ExitStatus, Error> {
    let excode = Command::new("chroot").arg(path).status();
    excode
}

// Clone tree
pub fn clone_as_tree(snapshot: &str, desc: &str) -> Result<i32, Error> {
    // Make sure snapshot does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot clone as snapshot {} doesn't exist.", snapshot)));

    } else {
        // Find the next available snapshot number
        let i = find_new()?;
        // Make snapshot mutable or immutable
        let immutability = if check_mutability(snapshot) {
           false
        } else {
           true
        };

        // Create snapshot
        create_snapshot(&format!("/.snapshots/boot/boot-{}", snapshot),
                        &format!("/.snapshots/boot/boot-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/etc/etc-{}", snapshot),
                        &format!("/.snapshots/etc/etc-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/var/var-{}", snapshot),
                        &format!("/.snapshots/var/var-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-{}", snapshot),
                        &format!("/.snapshots/rootfs/snapshot-{}", i),
                        immutability)?;

        // Mark newly created snapshot as mutable
        if !immutability {
            File::create(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/mutable", i))?;
        }

        // Import tree file
        let tree = fstree()?;
        // Add to root tree
        append_base_tree(&tree, i)?;
        // Save tree to fstree
        write_tree(&tree)?;
        // Write description for snapshot
        if desc.is_empty() {
            let description = format!("clone of {}.", snapshot);
            write_desc(&i.to_string(), &description, true)?;
        } else {
            write_desc(&i.to_string(), &desc, true)?;
        }
        Ok(i)
    }
}

// Clone branch under same parent
pub fn clone_branch(snapshot: &str) -> Result<i32, Error> {
    // Make sure snapshot does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot clone as snapshot {} doesn't exist.", snapshot)));

    } else {
        // Find the next available snapshot number
        let i = find_new()?;
        // Make snapshot mutable or immutable
        let immutability = if check_mutability(snapshot) {
           false
        } else {
           true
        };

        // Create snapshot
        create_snapshot(&format!("/.snapshots/boot/boot-{}", snapshot),
                        &format!("/.snapshots/boot/boot-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/etc/etc-{}", snapshot),
                        &format!("/.snapshots/etc/etc-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/var/var-{}", snapshot),
                        &format!("/.snapshots/var/var-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-{}", snapshot),
                        &format!("/.snapshots/rootfs/snapshot-{}", i),
                        immutability)?;

        // Mark newly created snapshot as mutable
        if !immutability {
            File::create(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/mutable", i))?;
        }

        // Import tree file
        let tree = fstree()?;
        // Clone within node
        add_node_to_level(&tree, snapshot, i)?;
        // Save tree to fstree
        write_tree(&tree)?;
        // Write description for snapshot
        let desc = format!("clone of {}.", snapshot);
        write_desc(&i.to_string(), &desc, true)?;
        Ok(i)
    }
}

// Recursively clone an entire tree
pub fn clone_recursive(snapshot: &str) -> Result<(), Error> {
    // Make sure snapshot does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot clone as snapshot {} doesn't exist.", snapshot)));

    } else {
        // Import tree file
        let tree = fstree()?;
        // Clone its branch and replace the original child with the clone
        let mut children = return_children(&tree, snapshot)?;
        let ch = children.clone();
        children.insert(0, snapshot.to_string());
        let ntree = clone_branch(snapshot)?;
        let mut new_children = ch.clone();
        new_children.insert(0, ntree.to_string());

        // Clone each child's branch under the corresponding parent in the new children list
        for child in ch {
            let parent = get_parent(&tree, &child)?.to_string();
            let index = children.iter().position(|x| x == &parent)
                .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Parent {} not found in tree.", parent)))?;
            let i = clone_under(&new_children[index], &child)?;
            // Record the new id at the CHILD's position so the next child
            // of the same parent attaches to the clone, not to the
            // original node still in the list
            let child_index = children.iter().position(|x| x == &child)
                .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Child {} not found in tree.", child)))?;
            new_children[child_index] = i.to_string();
        }
    }
    Ok(())
}

// Clone under specified parent
pub fn clone_under(snapshot: &str, branch: &str) -> Result<i32, Error> {
    // Make sure snapshot does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot clone as snapshot {} doesn't exist.", snapshot)));

        // Make sure branch does exist
        } else if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", branch)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot clone as snapshot {} doesn't exist.", branch)));

    } else {

        // Find the next available snapshot number
        let i = find_new()?;
        // Check mutability
        let immutability = if check_mutability(branch) {
           false
        } else {
           true
        };

        // Create snapshot: the new branch is a copy of branch, attached
        // under snapshot
        create_snapshot(&format!("/.snapshots/boot/boot-{}", branch),
                        &format!("/.snapshots/boot/boot-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/etc/etc-{}", branch),
                        &format!("/.snapshots/etc/etc-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/var/var-{}", branch),
                        &format!("/.snapshots/var/var-{}", i),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-{}", branch),
                        &format!("/.snapshots/rootfs/snapshot-{}", i),
                        immutability)?;

        // Mark newly created snapshot as mutable
        if !immutability {
            File::create(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/mutable", i))?;
        }

        // Import tree file
        let tree = fstree()?;
        // Add child to node
        add_node_to_parent(&tree, snapshot, i)?;
        // Save tree to fstree
        write_tree(&tree)?;
        // Write description for snapshot
        let desc = format!("clone of {}.", branch);
        write_desc(&i.to_string(), &desc, true)?;
        Ok(i)
    }
}

// Everything after '#' is a comment
fn comment_after_hash(line: &mut String) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

// Delete tree or branch
pub fn delete_node(snapshots: &Vec<String>, quiet: bool, nuke: bool) -> Result<(), Error> {
    // Get some values
    let current_snapshot = get_current_snapshot()?;
    let next_snapshot = get_next_snapshot()?;
    let mut run = false;

    // Iterating over snapshots
    for snapshot in snapshots {
        // Make sure snapshot is not base snapshot
        if snapshot.as_str() == "0" {
            return Err(Error::new(ErrorKind::Unsupported, format!("Changing base snapshot is not allowed.")));

        // Make sure snapshot does exist
        } else if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
            return Err(Error::new(ErrorKind::NotFound, format!(
                "Cannot delete as snapshot {} doesn't exist.", snapshot)));
        }

        if !nuke {
            // Make sure snapshot is not current working snapshot
            if snapshot == &current_snapshot {
                return Err(Error::new(ErrorKind::Unsupported, format!(
                    "Cannot delete booted snapshot.")));
            // Make sure snapshot is not deploy snapshot
            } else if snapshot == &next_snapshot {
                return Err(Error::new(ErrorKind::Unsupported, format!(
                    "Cannot delete deployed snapshot.")));

            // Abort if not quiet and confirmation message is false
            } else if !quiet && !yes_no(&format!("Are you sure you want to delete snapshot {}?", snapshot)) {
                return Err(Error::new(ErrorKind::Interrupted, format!(
                    "Aborted.")));
            } else {
                run = true;
            }
        }

        if nuke | run {
            // Delete snapshot
            let tree = fstree()?;
            let children = return_children(&tree, &snapshot)?;
            let desc_path = format!("/.snapshots/dsnap/snapshots/{}-desc", snapshot);
            if Path::new(&desc_path).is_file() {
                std::fs::remove_file(desc_path)?;
            }
            if Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
                delete_subvolume(&format!("/.snapshots/boot/boot-{}", snapshot))?;
                delete_subvolume(&format!("/.snapshots/etc/etc-{}", snapshot))?;
                delete_subvolume(&format!("/.snapshots/var/var-{}", snapshot))?;
                delete_subvolume(&format!("/.snapshots/rootfs/snapshot-{}", snapshot))?;
            }

            // Make sure temporary chroot directories are deleted as well
            if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
                delete_subvolume(&format!("/.snapshots/boot/boot-chr{}", snapshot))?;
                delete_subvolume(&format!("/.snapshots/etc/etc-chr{}", snapshot))?;
                delete_subvolume(&format!("/.snapshots/var/var-chr{}", snapshot))?;
                delete_subvolume(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))?;
            }

            for child in children {
                // This deletes the node itself along with its children
                let desc_path = format!("/.snapshots/dsnap/snapshots/{}-desc", child);
                if Path::new(&desc_path).is_file() {
                    std::fs::remove_file(desc_path)?;
                }
                if Path::new(&format!("/.snapshots/rootfs/snapshot-{}", child)).try_exists()? {
                    delete_subvolume(&format!("/.snapshots/boot/boot-{}", child))?;
                    delete_subvolume(&format!("/.snapshots/etc/etc-{}", child))?;
                    delete_subvolume(&format!("/.snapshots/var/var-{}", child))?;
                    delete_subvolume(&format!("/.snapshots/rootfs/snapshot-{}", child))?;
                }
                if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", child)).try_exists()? {
                    delete_subvolume(&format!("/.snapshots/boot/boot-chr{}", child))?;
                    delete_subvolume(&format!("/.snapshots/etc/etc-chr{}", child))?;
                    delete_subvolume(&format!("/.snapshots/var/var-chr{}", child))?;
                    delete_subvolume(&format!("/.snapshots/rootfs/snapshot-chr{}", child))?;
                }
            }

            // Remove node from tree or root
            remove_node(&tree, snapshot)?;
            write_tree(&tree)?;
        }
    }
    Ok(())
}

// Deploy snapshot
pub fn deploy(snapshot: &str, secondary: bool, reset: bool) -> Result<String, Error> {
    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot deploy as snapshot {} doesn't exist.", snapshot)));

    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        // Make sure snapshot is not in use by another dsnap process
        return Err(
            Error::new(
                ErrorKind::Unsupported,
                format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                        snapshot,snapshot)));
    } else if Path::new("/.snapshots/rootfs/snapshot-chr0").try_exists()? {
        // Make sure base snapshot is not in use by another dsnap process
        return Err(
            Error::new(
                ErrorKind::Unsupported,
                "Snapshot 0 appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s 0'."));

    } else {
        // Create rollback snapshot
        let tmp = get_tmp();
        if Path::new("/.snapshots/rootfs/snapshot-rollback").try_exists()? {
            delete_subvolume("/.snapshots/rootfs/snapshot-rollback")?;
            delete_subvolume("/.snapshots/boot/boot-rollback")?;
            delete_subvolume("/.snapshots/etc/etc-rollback")?;
            delete_subvolume("/.snapshots/var/var-rollback")?;
        }
        create_snapshot(&format!("/.snapshots/boot/boot-{}", tmp),
                        "/.snapshots/boot/boot-rollback",
                        false)?;
        create_snapshot(&format!("/.snapshots/etc/etc-{}", tmp),
                        "/.snapshots/etc/etc-rollback",
                        false)?;
        create_snapshot(&format!("/.snapshots/var/var-{}", tmp),
                        "/.snapshots/var/var-rollback",
                        false)?;
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-{}", tmp),
                        "/.snapshots/rootfs/snapshot-rollback",
                        false)?;
        remove_dir_content("/.snapshots/rootfs/snapshot-rollback/var/tmp")?;

        // Update bootloader
        update_boot(snapshot, secondary)?;

        // Set default volume
        #[cfg(feature = "btrfs")]
        btrfs::set_default(&format!("/.snapshots/rootfs/snapshot-{}", tmp))?;

        // Delete old tmp
        tmp_delete(secondary)?;
        // Get new tmp
        let tmp = get_aux_tmp(tmp, secondary);

        // Special mutable directories
        let options = snapshot_config_get(snapshot);
        let mutable_dirs: Vec<&str> = options.get("mutable_dirs")
                                             .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                 if let Some(index) = dir.find("::") {
                                                     vec![&dir[..index], &dir[index + 2..]]
                                                 } else {
                                                     vec![dir]
                                                 }
                                             }).filter(|dir| !dir.trim().is_empty()).collect()})
                                             .unwrap_or_else(|| Vec::new());
        let mutable_dirs_shared: Vec<&str> = options.get("mutable_dirs_shared")
                                                    .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                        if let Some(index) = dir.find("::") {
                                                            vec![&dir[..index], &dir[index + 2..]]
                                                        } else {
                                                            vec![dir]
                                                        }
                                                    }).filter(|dir| !dir.trim().is_empty()).collect()})
                                                    .unwrap_or_else(|| Vec::new());

        // Snapshot operations
        create_snapshot(&format!("/.snapshots/boot/boot-{}", snapshot),
                        &format!("/.snapshots/boot/boot-{}", tmp),
                        false)?;
        create_snapshot(&format!("/.snapshots/etc/etc-{}", snapshot),
                        &format!("/.snapshots/etc/etc-{}", tmp),
                        false)?;
        create_snapshot(&format!("/.snapshots/var/var-{}", snapshot),
                        &format!("/.snapshots/var/var-{}", tmp),
                        false)?;
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-{}", snapshot),
                        &format!("/.snapshots/rootfs/snapshot-{}", tmp),
                        false)?;
        DirBuilder::new().recursive(true)
                         .create(format!("/.snapshots/rootfs/snapshot-{}/boot", tmp))?;
        DirBuilder::new().recursive(true)
                         .create(format!("/.snapshots/rootfs/snapshot-{}/etc", tmp))?;
        DirBuilder::new().recursive(true)
                         .create(format!("/.snapshots/rootfs/snapshot-{}/var", tmp))?;
                cp_reflink(format!("/.snapshots/boot/boot-{}/.", snapshot),
                   format!("/.snapshots/rootfs/snapshot-{}/boot", tmp))?;
                cp_reflink(format!("/.snapshots/etc/etc-{}/.", snapshot),
                   format!("/.snapshots/rootfs/snapshot-{}/etc", tmp))?;
                cp_reflink(format!("/.snapshots/var/var-{}/.", snapshot),
                   format!("/.snapshots/rootfs/snapshot-{}/var", tmp))?;

        // If snapshot is mutable, modify '/' entry in fstab to read-write
        if check_mutability(snapshot) {
            let mut fstab_file = File::open(format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp))?;
            let mut contents = String::new();
            fstab_file.read_to_string(&mut contents)?;

            // The root entry in the copied fstab still references the
            // old deployment slot; remove ",ro" from the mount options of
            // the root filesystem (mount point "/") instead of searching
            // for the not-yet-renamed slot name
            let mut new_lines: Vec<String> = Vec::new();
            for line in contents.lines() {
                let trimmed = line.trim_start();
                let is_root_mount = !trimmed.starts_with('#')
                    && line.split_whitespace().nth(1) == Some("/");
                if is_root_mount && line.contains(",ro") {
                    new_lines.push(line.replace(",ro", ""));
                } else {
                    new_lines.push(line.to_string());
                }
            }
            let mut new_contents = new_lines.join("\n");
            if contents.ends_with('\n') {
                new_contents.push('\n');
            }
            std::fs::write(format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp), new_contents)?;
        }

        // Add special user-defined mutable directories as bind-mounts into fstab
        if !mutable_dirs.is_empty() {
            for mount_path in mutable_dirs {
                let source_path = format!("/.snapshots/mutable_dirs/snapshot-{}/{}", snapshot,mount_path);
                DirBuilder::new().recursive(true)
                                 .create(format!("/.snapshots/mutable_dirs/snapshot-{}/{}", snapshot,mount_path))?;
                DirBuilder::new().recursive(true)
                                 .create(format!("/.snapshots/rootfs/snapshot-{}/{}", tmp,mount_path))?;
                let fstab = format!("{} /{} none defaults,bind 0 0", source_path,mount_path);
                let mut fstab_file = OpenOptions::new().append(true)
                                                       .create(true)
                                                       .read(true)
                                                       .open(format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp))?;
                fstab_file.write_all(format!("{}\n", fstab).as_bytes())?;
            }
        }

        // Same thing but for shared directories
        if !mutable_dirs_shared.is_empty() {
            for mount_path in mutable_dirs_shared {
                let source_path = format!("/.snapshots/mutable_dirs/{}", mount_path);
                DirBuilder::new().recursive(true)
                                 .create(format!("/.snapshots/mutable_dirs/{}", mount_path))?;
                DirBuilder::new().recursive(true)
                                 .create(format!("/.snapshots/rootfs/snapshot-{}/{}", tmp,mount_path))?;
                let fstab = format!("{} /{} none defaults,bind 0 0", source_path,mount_path);
                let mut fstab_file = OpenOptions::new().append(true)
                                                       .create(true)
                                                       .read(true)
                                                       .open(format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp))?;
                fstab_file.write_all(format!("{}\n", fstab).as_bytes())?;
            }
        }

        let snap_num = format!("{}", snapshot);
        let mut snap_file = OpenOptions::new().truncate(true)
                                              .create(true)
                                              .read(true)
                                              .write(true)
                                              .open(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/snap", tmp))?;
        snap_file.write_all(snap_num.as_bytes())?;
        let target_dep = switch_tmp(snapshot, secondary, reset)?;

        // Set default volume
        #[cfg(feature = "btrfs")]
        btrfs::set_default(&format!("/.snapshots/rootfs/snapshot-{}", tmp))?;

        Ok(target_dep)
    }
}

// Deploy recovery snapshot
pub fn deploy_recovery(snapshot: &str) -> Result<(), Error> {
    let tmp = get_recovery_tmp();

    // Update boot
    update_boot("0", false)?;

    // Special mutable directories
    let options = snapshot_config_get("0");
    let mutable_dirs: Vec<&str> = options.get("mutable_dirs")
                                         .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                             if let Some(index) = dir.find("::") {
                                                 vec![&dir[..index], &dir[index + 2..]]
                                             } else {
                                                 vec![dir]
                                             }
                                         }).filter(|dir| !dir.trim().is_empty()).collect()})
                                         .unwrap_or_else(|| Vec::new());
    let mutable_dirs_shared: Vec<&str> = options.get("mutable_dirs_shared")
                                                .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                    if let Some(index) = dir.find("::") {
                                                        vec![&dir[..index], &dir[index + 2..]]
                                                    } else {
                                                        vec![dir]
                                                    }
                                                }).filter(|dir| !dir.trim().is_empty()).collect()})
                                                .unwrap_or_else(|| Vec::new());

    // Change recovery tmp
    let tmp = get_recovery_aux_tmp(&tmp);

    // Clean tmp
    if Path::new(&format!("/.snapshots/rootfs/snapshot-{}", tmp)).try_exists()? {
        delete_subvolume(&format!("/.snapshots/boot/boot-{}", tmp))?;
        delete_subvolume(&format!("/.snapshots/etc/etc-{}", tmp))?;
        delete_subvolume(&format!("/.snapshots/var/var-{}", tmp))?;
        delete_subvolume(&format!("/.snapshots/rootfs/snapshot-{}", tmp))?;
    }

    // Snapshot operations
    create_snapshot("/.snapshots/boot/boot-0",
                    &format!("/.snapshots/boot/boot-{}", tmp),
                    false)?;
    create_snapshot("/.snapshots/etc/etc-0",
                    &format!("/.snapshots/etc/etc-{}", tmp),
                    false)?;
    create_snapshot("/.snapshots/var/var-0",
                    &format!("/.snapshots/var/var-{}", tmp),
                    false)?;
    create_snapshot("/.snapshots/rootfs/snapshot-0",
                    &format!("/.snapshots/rootfs/snapshot-{}", tmp),
                    false)?;
    DirBuilder::new().recursive(true)
                     .create(format!("/.snapshots/rootfs/snapshot-{}/boot", tmp))?;
    DirBuilder::new().recursive(true)
                     .create(format!("/.snapshots/rootfs/snapshot-{}/etc", tmp))?;
    DirBuilder::new().recursive(true)
                     .create(format!("/.snapshots/rootfs/snapshot-{}/var", tmp))?;
        cp_reflink("/.snapshots/boot/boot-0/.",
               format!("/.snapshots/rootfs/snapshot-{}/boot", tmp))?;
        cp_reflink("/.snapshots/etc/etc-0/.",
               format!("/.snapshots/rootfs/snapshot-{}/etc", tmp))?;
        cp_reflink("/.snapshots/var/var-0/.",
               format!("/.snapshots/rootfs/snapshot-{}/var", tmp))?;

    // Update fstab for new deployment
    let fstab_file = format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp);
    // Read the contents of the file into a string
    let mut contents = String::new();
    let mut file = File::open(&fstab_file)?;
    file.read_to_string(&mut contents)?;

    let src_tmp = if contents.contains("deploy-aux") && !contents.contains("secondary") {
        "deploy-aux"
    } else if contents.contains("secondary") && !contents.contains("aux") {
        "deploy-secondary"
    } else if contents.contains("aux-secondary") {
        "deploy-aux-secondary"
    } else {
        "deploy"
    };

    let modified_boot_contents = contents.replace(&format!("@.snapshots_linux/boot/boot-{}", src_tmp),
                                                  &format!("@.snapshots_linux/boot/boot-{}", tmp));
    let modified_etc_contents = modified_boot_contents.replace(&format!("@.snapshots_linux/etc/etc-{}", src_tmp),
                                                               &format!("@.snapshots_linux/etc/etc-{}", tmp));
    let modified_var_contents = modified_etc_contents.replace(&format!("@.snapshots_linux/var/var-{}", src_tmp),
                                                               &format!("@.snapshots_linux/var/var-{}", tmp));
    let modified_rootfs_contents = modified_var_contents.replace(&format!("@.snapshots_linux/rootfs/snapshot-{}", src_tmp),
                                                                 &format!("@.snapshots_linux/rootfs/snapshot-{}", tmp));

    // Write the modified contents back to the file
    let mut file = File::create(fstab_file)?;
    file.write_all(modified_rootfs_contents.as_bytes())?;

    // If snapshot is mutable, modify '/' entry in fstab to read-write
    if check_mutability("0") {
        let mut fstab_file = File::open(format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp))?;
        let mut contents = String::new();
        fstab_file.read_to_string(&mut contents)?;

        let pattern = format!("snapshot-{}", tmp);
        if let Some(index) = contents.find(&pattern) {
            if let Some(end) = contents[index..].find(",ro") {
                let replace_index = index + end;
                let mut new_contents = String::with_capacity(contents.len());
                new_contents.push_str(&contents[..replace_index]);
                new_contents.push_str(&contents[replace_index + 3..]);
                std::fs::write(format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp), new_contents)?;
            }
        }
    }

    // Add special user-defined mutable directories as bind-mounts into fstab
    if !mutable_dirs.is_empty() {
        for mount_path in mutable_dirs {
            let source_path = format!("/.snapshots/mutable_dirs/snapshot-0/{}", mount_path);
            DirBuilder::new().recursive(true)
                             .create(format!("/.snapshots/mutable_dirs/snapshot-0/{}", mount_path))?;
            DirBuilder::new().recursive(true)
                             .create(format!("/.snapshots/rootfs/snapshot-{}/{}", tmp,mount_path))?;
            let fstab = format!("{} /{} none defaults,bind 0 0", source_path,mount_path);
            let mut fstab_file = OpenOptions::new().append(true)
                                                   .create(true)
                                                   .read(true)
                                                   .open(format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp))?;
            fstab_file.write_all(format!("{}\n", fstab).as_bytes())?;
        }
    }

    // Same thing but for shared directories
    if !mutable_dirs_shared.is_empty() {
        for mount_path in mutable_dirs_shared {
            let source_path = format!("/.snapshots/mutable_dirs/{}", mount_path);
            DirBuilder::new().recursive(true)
                             .create(format!("/.snapshots/mutable_dirs/{}", mount_path))?;
            DirBuilder::new().recursive(true)
                             .create(format!("/.snapshots/rootfs/snapshot-{}/{}", tmp,mount_path))?;
            let fstab = format!("{} /{} none defaults,bind 0 0", source_path,mount_path);
            let mut fstab_file = OpenOptions::new().append(true)
                                                   .create(true)
                                                   .read(true)
                                                   .open(format!("/.snapshots/rootfs/snapshot-{}/etc/fstab", tmp))?;
            fstab_file.write_all(format!("{}\n", fstab).as_bytes())?;
        }
    }

    let snap_num = "0";
    let mut snap_file = OpenOptions::new().truncate(true)
                                          .create(true)
                                          .read(true)
                                          .write(true)
                                          .open(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/snap", tmp))?;
    snap_file.write_all(snap_num.as_bytes())?;

    // Update recovery tmp
    switch_recovery_tmp(&snap_num, snapshot)?;
    prepare("0")?;
    let mut recovery_tmp = OpenOptions::new().truncate(true)
                                             .create(true)
                                             .read(true)
                                             .write(true)
                                             .open("/.snapshots/rootfs/snapshot-chr0/usr/share/dsnap/rec-tmp")?;
    recovery_tmp.write_all(tmp.as_bytes())?;
    post_transactions("0")?;

    Ok(())
}

// Show diff of packages
pub fn diff(snapshot1: &str, snapshot2: &str) -> Result<(), Error> {
    snapshot_diff(snapshot1, snapshot2)
}

// Switch between installed distributions by editing the EFI boot order
pub fn efi_boot_order() -> Result<(), Error> {
    // Get efibootmgr output
    let efibootmgr_output = Command::new("efibootmgr").output()?;
    if !efibootmgr_output.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to run efibootmgr: {}",
                                      String::from_utf8_lossy(&efibootmgr_output.stderr).trim())));
    }
    let efibootmgr_str = String::from_utf8_lossy(&efibootmgr_output.stdout);

    // Parse boot entries, lines look like:
    //   Boot0004* dsnap
    //   Boot0001  Windows Boot Manager
    let mut entries: Vec<(String, String)> = Vec::new(); // (boot number, label)
    for line in efibootmgr_str.lines() {
        if let Some(rest) = line.strip_prefix("Boot") {
            if rest.len() >= 6
               && rest[..4].chars().all(|c| c.is_ascii_hexdigit())
               && (rest.as_bytes()[4] == b'*' || rest.as_bytes()[4] == b' ') {
                let bootnum = rest[..4].to_string();
                let label = rest[5..].trim().to_string();
                entries.push((bootnum, label));
            }
        }
    }

    if entries.is_empty() {
        return Err(Error::new(ErrorKind::Other, "No EFI boot entries found."));
    }

    // Parse the current boot order, falls back to the entry order if missing
    let boot_order: Vec<String> = efibootmgr_str.lines()
        .find(|line| line.starts_with("BootOrder: "))
        .map(|line| line["BootOrder: ".len()..]
             .split(',')
             .map(|num| num.trim().to_string())
             .collect())
        .unwrap_or_else(|| entries.iter().map(|(num, _)| num.clone()).collect());

    loop {
        // List the entries
        for (num, label) in &entries {
            println!("{} ({})", label, num);
        }

        println!("Type the name or boot number of a distribution to switch to: (type 'list' to list them, 'q' to quit)");
        let mut next_distro = String::new();
        stdin().read_line(&mut next_distro)?;
        let next_distro = next_distro.trim();

        if next_distro.eq_ignore_ascii_case("q") {
            break;
        } else if next_distro.eq_ignore_ascii_case("list") {
            continue;
        }

        // Find the entry by label or boot number
        let found = entries.iter()
            .find(|(num, label)| label.eq_ignore_ascii_case(next_distro) || num.eq_ignore_ascii_case(next_distro));

        match found {
            Some((bootnum, label)) => {
                // Move the target entry to the front, keep the rest of the order
                let mut new_boot_order: Vec<String> = vec![bootnum.clone()];
                for num in &boot_order {
                    if num != bootnum {
                        new_boot_order.push(num.clone());
                    }
                }
                let new_boot_order = new_boot_order.join(",");

                let efibootmgr_output = Command::new("efibootmgr")
                    .args(["--bootorder", &new_boot_order])
                    .output()?;

                if efibootmgr_output.status.success() {
                    println!("Done! Please reboot whenever you would like to switch to {}.", label);
                    break;
                } else {
                    eprintln!("Failed to switch boot order: {}",
                              String::from_utf8_lossy(&efibootmgr_output.stderr).trim());
                }
            },
            None => eprintln!("Invalid distribution!"),
        }
    }
    Ok(())
}

// Send snapshot to export directory
pub fn export(snapshot: &str, dest: &str) -> Result<(), Error> {
    // Get current time
    let time = Local::now().naive_local();
    let formatted = time.format("%Y%m%d-%H%M%S").to_string();

    // Send btrfs to destination
    #[cfg(feature = "btrfs")]
    {
        // The destination is passed as $1 so paths with spaces or shell
        // metacharacters survive, and the pipeline status is checked:
        // otherwise a failed btrfs send was silently ignored whenever
        // zstd exited successfully
        let excode = Command::new("sh").arg("-c")
                       .arg(format!("sudo btrfs send /.snapshots/rootfs/snapshot-{} | zstd -o \"$1/snapshot-{}.{}.zst\"",
                                    snapshot, snapshot, formatted))
                       .arg("sh")
                       .arg(dest)
                       .status()?;
        if !excode.success() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to export snapshot {} to {}.", snapshot, dest)));
        }
    }

    Ok(())
}

// Find new unused snapshot dir
pub fn find_new() -> Result<i32, Error> {
    fn collect(dir: &str) -> Result<Vec<PathBuf>, Error> {
        read_dir(dir)?
            .map(|entry| entry.map(|e| e.path()))
            .collect()
    }

    let mut snapshots = collect("/.snapshots/rootfs")?;
    snapshots.extend(collect("/.snapshots/var")?);
    snapshots.extend(collect("/.snapshots/etc")?);
    snapshots.extend(collect("/.snapshots/boot")?);

    let mut i = 0;
    loop {
        i += 1;
        if !snapshots.contains(&PathBuf::from(format!("/.snapshots/rootfs/snapshot-{}", i)))
            && !snapshots.contains(&PathBuf::from(format!("/.snapshots/var/var-{}", i)))
            && !snapshots.contains(&PathBuf::from(format!("/.snapshots/etc/etc-{}", i)))
            && !snapshots.contains(&PathBuf::from(format!("/.snapshots/boot/boot-{}", i)))
        {
            return Ok(i);
        }
    }
}

// FixDB
pub fn fixdb(_snapshot: &str) -> Result<(), Error> {
    #[cfg(feature = "pacman")]
    fix_package_db(_snapshot)?;
    Ok(())
}

// Get aux tmp
pub fn get_aux_tmp(tmp: String, secondary: bool) -> String {
    let tmp = if secondary {
        if tmp == "deploy-aux-secondary" {
            tmp.replace("deploy-aux-secondary", "deploy-secondary")
        } else if tmp == "deploy-aux" {
            tmp.replace("deploy-aux", "deploy-aux-secondary")
        } else if tmp == "deploy" {
            tmp.replace("deploy", "deploy-secondary")
        } else {
            tmp.replace("deploy-secondary", "deploy-aux-secondary")
        }
    } else {
        if tmp == "deploy-aux" {
            tmp.replace("deploy-aux", "deploy")
        } else if tmp == "deploy-aux-secondary" {
            tmp.replace("deploy-aux-secondary", "deploy-aux")
        } else if tmp == "deploy-secondary" {
            tmp.replace("deploy-secondary", "deploy")
        } else {
            tmp.replace("deploy", "deploy-aux")
        }
    };
    tmp
}

// Get current snapshot
pub fn get_current_snapshot() -> Result<String, Error> {
    let csnapshot = read_to_string("/usr/share/dsnap/snap")?;
    Ok(csnapshot.trim_end().to_string())
}

// Get snapshot name in tmp
fn get_import_snapshot_name(path: &Path) -> Result<String, Error> {
    for entry in WalkDir::new(path).max_depth(1) {
        let entry = entry.map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
            if name.contains("snapshot-") {
                return Ok(name.to_string());
            }
        }
    }
    Err(Error::new(
        ErrorKind::NotFound,
        format!("Could not find received snapshot in {}.", path.display()),
    ))
}

// Get deployed snapshot
pub fn get_next_snapshot() -> Result<String, Error> {
    let tmp = get_tmp();
    let file = match File::open("/.snapshots/dsnap/deploy-tmp") {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(e),
    };
    let mut d = String::new();
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        if line != tmp {
            d.push_str(&line);
            break;
        }
    }
    if d.is_empty() {
        return Ok(String::new());
    }

    // Make sure next snapshot exists
    let snap_path = format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/snap", d);
    Ok(read_to_string(&snap_path)?.trim().to_string())
}

// Get drive partition
pub fn get_part() -> Result<String, Error> {
    // Read part UUID
    let mut file = File::open("/.snapshots/dsnap/part")?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    // Get partition path from UUID
    let uuid = contents.trim_end().to_string();
    let cpart = PartitionID::new(PartitionSource::UUID, uuid.clone());
    match cpart.get_device_path() {
        Some(path) => Ok(path.to_string_lossy().into_owned()),
        None => Err(Error::new(ErrorKind::NotFound,
                               format!("Could not resolve partition with UUID {}.", uuid))),
    }
}

// Get recovery aux tmp
pub fn get_recovery_aux_tmp(tmp: &str) -> String {
    let tmp = if tmp == "recovery-deploy-aux" {
        tmp.replace("recovery-deploy-aux", "recovery-deploy")
    } else {
        tmp.replace("recovery-deploy", "recovery-deploy-aux")
    };
    tmp
}

// Get recovery tmp state
pub fn get_recovery_tmp() -> String {
    let path = "/.snapshots/rootfs/snapshot-0/usr/share/dsnap/rec-tmp";
    if Path::new(path).is_file() {
        if let Ok(contents) = read_to_string(path) {
            let tmp = contents.trim().to_string();
            if !tmp.is_empty() {
                return tmp;
            }
        }
    }
    "recovery-deploy".to_string()
}

// Get tmp partition state
pub fn get_tmp() -> String {
    // By default just return which deployment is running
    let mounts = read_to_string("/proc/mounts").unwrap_or_default();
    let mount: Vec<&str> = mounts
        .lines()
        .filter(|line| {
            #[cfg(feature = "btrfs")]
            { line.contains(" / btrfs") }
            #[cfg(not(feature = "btrfs"))]
            { let _ = line; false }
        })
        .collect();
    // Check recovery deployments first so they are not mistaken for
    // regular deploy slots by substring matches
    if mount.iter().any(|element| element.contains("recovery-deploy-aux")) {
        "recovery-deploy-aux".to_string()
    } else if mount.iter().any(|element| element.contains("recovery-deploy")) {
        "recovery-deploy".to_string()
    } else if mount.iter().any(|element| element.contains("deploy-aux-secondary")) {
        "deploy-aux-secondary".to_string()
    } else if mount.iter().any(|element| element.contains("deploy-secondary")) {
        "deploy-secondary".to_string()
    } else if mount.iter().any(|element| element.contains("deploy-aux")) {
        "deploy-aux".to_string()
    } else {
        "deploy".to_string()
    }
}

// Make a snapshot vulnerable to be modified even further (snapshot should be deployed as mutable)
pub fn hollow(snapshot: &str) -> Result<(), Error> {
    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot make hollow as snapshot {} doesn't exist.", snapshot)));

        // Make sure snapshot is not in use by another dsnap process
        } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(
            Error::new(
                ErrorKind::Unsupported,
                format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                        snapshot,snapshot)));

        // Make sure snapshot is not  base snapshot
        } else if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported, format!("Changing base snapshot is not allowed.")));

    } else {
        prepare(snapshot)?;
        // Mount root
        #[cfg(feature = "btrfs")]
        mount(Some("/"), format!("/.snapshots/rootfs/snapshot-chr{}", snapshot).as_str(),
              Some("btrfs"), MsFlags::MS_BIND | MsFlags::MS_REC | MsFlags::MS_SLAVE, None::<&str>)?;
        // Deploy or not
        if yes_no(&format!("Snapshot {} is now hollow! Whenever done, type yes to deploy and no to discard.", snapshot)) {
            posttrans(snapshot)?;
            // A hollowed snapshot is meant to be editable, so ensure
            // it is mutable regardless of its previous state
            immutability_disable(snapshot)?;
            deploy(snapshot, false, false)?;
        } else {
            let teardown = hollow_teardown(snapshot);
            let cleanup = chr_delete(snapshot);
            teardown?;
            cleanup?;
            return Err(Error::new(ErrorKind::Interrupted,
                                  format!("No changes applied on snapshot {}.", snapshot)));
        }
    }
    Ok(())
}

// Make a node mutable
pub fn immutability_disable(snapshot: &str) -> Result<(), Error> {
    // If not base snapshot
    if snapshot != "0" {
        // Make sure snapshot exists
        if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
            return Err(Error::new(ErrorKind::NotFound, format!("Snapshot {} doesn't exist.", snapshot)));

        } else {
            // Make sure snapshot is not already mutable
            if check_mutability(snapshot) {
                return Err(Error::new(ErrorKind::AlreadyExists,
                                      format!("Snapshot {} is already mutable.", snapshot)));

            } else {
                // Disable immutability
                set_subvolume_read_only(&format!("/.snapshots/rootfs/snapshot-{}", snapshot), false)?;
                File::create(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/mutable", snapshot))?;
                write_desc(snapshot, " MUTABLE ", false)?;
            }
        }

    } else {
        // Base snapshot unsupported
        return Err(Error::new(ErrorKind::Unsupported, format!("Snapshot 0 (base) should not be modified.")));
    }
    Ok(())
}

//Make a node immutable
pub fn immutability_enable(snapshot: &str) -> Result<(), Error> {
    // If not base snapshot
    if snapshot != "0" {
        // Make sure snapshot exists
        if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
            return Err(Error::new(ErrorKind::NotFound, format!("Snapshot {} doesn't exist.", snapshot)));

        } else {
            // Make sure snapshot is not already immutable
            if !check_mutability(snapshot) {
                return Err(Error::new(ErrorKind::AlreadyExists,
                                      format!("Snapshot {} is already immutable.", snapshot)));
            } else {
                // Enable immutability
                std::fs::remove_file(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/mutable", snapshot))?;
                set_subvolume_read_only(&format!("/.snapshots/rootfs/snapshot-{}", snapshot), true)?;
                // Read the desc file into a string
                let mut contents = std::fs::read_to_string(format!("/.snapshots/dsnap/snapshots/{}-desc", snapshot))?;
                // Replace MUTABLE word with an empty string
                contents = contents.replace(" MUTABLE ", "");
                // Write the modified contents back to the file
                std::fs::write(format!("/.snapshots/dsnap/snapshots/{}-desc", snapshot), contents)?;
            }
        }

    } else {
        // Base snapshot unsupported
        return Err(Error::new(ErrorKind::Unsupported, format!("Snapshot 0 (base) should not be modified.")));
    }
    Ok(())
}

// Import snapshot
pub fn import(snapshot: i32, path: &str, desc: &str, tmp_dir: &TempDir) -> Result<(), Error> {
    // Make sure snapshot is not in use by another dsnap process
    if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(Error::new
                   (ErrorKind::Unsupported,
                    format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                            snapshot,snapshot)));

    // Make sure snapshot does exist
    } else if !Path::new(&format!("{}", path)).is_file() {
        return Err(Error::new(ErrorKind::NotFound, format!("The snapshot {} doesn't exist.", path)));

    } else {
        // AtomicBool flag to track if the process was interrupted
        let interrupted = Arc::new(AtomicBool::new(false));

        // Clone the Arc for use in the signal handler closure
        let interrupted_clone = Arc::clone(&interrupted);

        // Register the Ctrl+C signal handler
        ctrlc::set_handler(move || {
            // Set the interrupted flag to true if the signal is received
            interrupted_clone.store(true, Ordering::SeqCst);
        }).map_err(|e| Error::new(ErrorKind::Other,
                                format!("Error setting Ctrl+C handler: {}", e)))?;

        let tmp_path = tmp_dir.path().to_string_lossy().to_string();

        // Receive snapshot
        println!("Receiving a snapshot, please wait...");
        #[cfg(feature = "btrfs")]
        {
            // The path travels as $1 so spaces and metacharacters in the
            // import file survive, and the receive status is checked:
            // otherwise a partial receive was treated as a success and
            // the corrupt result imported
            let excode = Command::new("sh").arg("-c")
                           .arg(format!("btrfs receive -f \"$1\" {}", tmp_path))
                           .arg("sh")
                           .arg(path)
                           .status()?;
            if !excode.success() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to receive snapshot from {}.", path)));
            }
        }

        // Get snapshot name
        let export_snapshot = get_import_snapshot_name(tmp_dir.path())?;
        let received_path = format!("{}/{}", tmp_path, export_snapshot);

        // Prepare snapshot
        create_snapshot(&received_path,
                        &format!("/.snapshots/rootfs/snapshot-chr{}", snapshot),
                        false)?;

        // Delete the folder if the process was interrupted
        if interrupted.load(Ordering::SeqCst) {
            // Clean tmp
            if Path::new(&received_path).try_exists().unwrap_or(false) {
                let _ = delete_subvolume(&received_path);
            }
            std::process::exit(1);
        }

        // Clean tmp
        delete_subvolume(&received_path)?;

        // Copy fstab
                cp_reflink("/.snapshots/rootfs/snapshot-0/etc/fstab",
                   &format!("/.snapshots/rootfs/snapshot-chr{}/etc/fstab", snapshot))?;

        // Mount chroot
        dsnap_mounts(&format!("{}", snapshot), "chr")?;

        // Special mutable directories
        let options = snapshot_config_get(&format!("{}", snapshot));
        let mutable_dirs: Vec<&str> = options.get("mutable_dirs")
                                             .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                 if let Some(index) = dir.find("::") {
                                                     vec![&dir[..index], &dir[index + 2..]]
                                                 } else {
                                                     vec![dir]
                                                 }
                                             }).filter(|dir| !dir.trim().is_empty()).collect()})
                                             .unwrap_or_else(|| Vec::new());
        let mutable_dirs_shared: Vec<&str> = options.get("mutable_dirs_shared")
                                                    .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                        if let Some(index) = dir.find("::") {
                                                            vec![&dir[..index], &dir[index + 2..]]
                                                        } else {
                                                            vec![dir]
                                                        }
                                                    }).filter(|dir| !dir.trim().is_empty()).collect()})
                                                    .unwrap_or_else(|| Vec::new());
        if !mutable_dirs.is_empty() {
            // Clean old mutable_dirs
            if Path::new(&format!("/.snapshots/mutable_dirs/snapshot-{}", snapshot)).try_exists()? {
                remove_dir_content(&format!("/.snapshots/mutable_dirs/snapshot-{}", snapshot))?;
            }
            for mount_path in mutable_dirs {
                if allow_dir_mut(mount_path) {
                    // Create mouth_path directory in snapshot
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/mutable_dirs/snapshot-{}/{}", snapshot,mount_path))?;
                    // Create mouth_path directory in snapshot-chr
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/rootfs/snapshot-chr{}/{}", snapshot,mount_path))?;
                    // Use mount_path
                    #[cfg(feature = "btrfs")]
                    mount(Some(format!("/.snapshots/mutable_dirs/snapshot-{}/{}", snapshot,mount_path).as_str()),
                          format!("/.snapshots/rootfs/snapshot-chr{}/{}", snapshot,mount_path).as_str(),
                          Some("btrfs"), MsFlags::MS_BIND , None::<&str>)?;
                }
            }
        }
        if !mutable_dirs_shared.is_empty() {
            // Never wipe the shared store: it is persistent data bind-mounted
            // into the deployed system (e.g. /var) and shared by every
            // snapshot. Only ensure the configured subdirectories exist
            for mount_path in mutable_dirs_shared {
                if allow_dir_mut(mount_path) {
                    // Create mouth_path directory in snapshot
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/mutable_dirs/{}", mount_path))?;
                    // Create mouth_path directory in snapshot-chr
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/rootfs/snapshot-chr{}/{}", snapshot,mount_path))?;
                    // Use mount_path
                    #[cfg(feature = "btrfs")]
                    mount(Some(format!("/.snapshots/mutable_dirs/{}", mount_path).as_str()),
                          format!("/.snapshots/rootfs/snapshot-chr{}/{}", snapshot,mount_path).as_str(),
                          Some("btrfs"), MsFlags::MS_BIND , None::<&str>)?;
                }
            }
        }

        // File operations for snapshot-chr
        create_snapshot("/.snapshots/boot/boot-0",
                        &format!("/.snapshots/boot/boot-chr{}", snapshot),
                        false)?;
        create_snapshot("/.snapshots/etc/etc-0",
                        &format!("/.snapshots/etc/etc-chr{}", snapshot),
                        false)?;
        create_snapshot("/.snapshots/var/var-0",
                        &format!("/.snapshots/var/var-chr{}", snapshot),
                        false)?;

        // Copy dsnap related configurations
        if Path::new("/etc/systemd").try_exists()? {
            // Machine-id is a Systemd thing
            copy("/etc/machine-id", format!("/.snapshots/rootfs/snapshot-chr{}/etc/machine-id", snapshot))?;
        }
        DirBuilder::new().recursive(true)
                         .create(format!("/.snapshots/rootfs/snapshot-chr{}/.snapshots/dsnap", snapshot))?;
        copy("/.snapshots/dsnap/fstree", format!("/.snapshots/rootfs/snapshot-chr{}/.snapshots/dsnap/fstree", snapshot))?;

        // Check if dsnap exists
        if !Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}/usr/sbin/dsnap", snapshot)).try_exists()? {
            return Err(Error::new(ErrorKind::NotFound,
                                  "/usr/sbin/dsnap doesn't exist."));
        }

        // Copy from chroot directory back to read only snapshot directory
        if post_transactions(&format!("{}", snapshot)).is_err() {
            chr_delete(&format!("{}", snapshot))?;
        }

        // Import tree file
        let tree = fstree()?;
        // Add to root tree
        append_base_tree(&tree, snapshot)?;
        // Save tree to fstree
        write_tree(&tree)?;
        // Write description for snapshot
        if desc.is_empty() {
            let description = format!("imported snapshot");
            write_desc(&snapshot.to_string(), &description, true)?;
        } else {
            write_desc(&snapshot.to_string(), &desc, true)?;
        }
    }
    Ok(())
}

// Import base snapshot
pub fn import_base(path: &str, tmp_dir: &TempDir) -> Result<String, Error> {
    let msg = "This process will change your base snapshot. Are you certain that you wish to proceed?";

    // Make sure snapshot is not in use by another dsnap process
    if Path::new("/.snapshots/rootfs/snapshot-chr0").try_exists()? {
        return Err(Error::new
                   (ErrorKind::Unsupported,
                    "Snapshot 0 appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s 0'."));

    // Make sure snapshot does exist
    } else if !Path::new(&format!("{}", path)).is_file() {
        return Err(Error::new(ErrorKind::NotFound, format!("The snapshot {} doesn't exist.", path)));

    } else if yes_no(msg) {
        // AtomicBool flag to track if the process was interrupted
        let interrupted = Arc::new(AtomicBool::new(false));

        // Clone the Arc for use in the signal handler closure
        let interrupted_clone = Arc::clone(&interrupted);

        // Register the Ctrl+C signal handler
        ctrlc::set_handler(move || {
            // Set the interrupted flag to true if the signal is received
            interrupted_clone.store(true, Ordering::SeqCst);
        }).map_err(|e| Error::new(ErrorKind::Other,
                                format!("Error setting Ctrl+C handler: {}", e)))?;

        let tmp_path = tmp_dir.path().to_string_lossy().to_string();

        // Receive snapshot
        println!("Receiving a snapshot, please wait...");
        #[cfg(feature = "btrfs")]
        {
            // The path travels as $1 so spaces and metacharacters in the
            // import file survive, and the receive status is checked:
            // otherwise a partial receive was treated as a success and
            // the corrupt result imported
            let excode = Command::new("sh").arg("-c")
                           .arg(format!("btrfs receive -f \"$1\" {}", tmp_path))
                           .arg("sh")
                           .arg(path)
                           .status()?;
            if !excode.success() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to receive snapshot from {}.", path)));
            }
        }

        // Get snapshot name
        let snapshot = get_import_snapshot_name(tmp_dir.path())?;
        let received_path = format!("{}/{}", tmp_path, snapshot);

        // Prepare snapshot
        create_snapshot(&received_path,
                        "/.snapshots/rootfs/snapshot-chr0",
                        false)?;

        // Delete the folder if the process was interrupted
        if interrupted.load(Ordering::SeqCst) {
            // Clean tmp
            if Path::new(&received_path).try_exists().unwrap_or(false) {
                let _ = delete_subvolume(&received_path);
            }
            std::process::exit(1);
        }

        // Clean tmp
        delete_subvolume(&received_path)?;
        clean_cache("0")?;

        // Copy fstab
                cp_reflink("/.snapshots/rootfs/snapshot-0/etc/fstab",
                   "/.snapshots/rootfs/snapshot-chr0/etc/fstab")?;

        // Mount chroot
        dsnap_mounts("0", "chr")?;

        // Special mutable directories
        let options = snapshot_config_get("0");
        let mutable_dirs: Vec<&str> = options.get("mutable_dirs")
                                             .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                 if let Some(index) = dir.find("::") {
                                                     vec![&dir[..index], &dir[index + 2..]]
                                                 } else {
                                                     vec![dir]
                                                 }
                                             }).filter(|dir| !dir.trim().is_empty()).collect()})
                                             .unwrap_or_else(|| Vec::new());
        let mutable_dirs_shared: Vec<&str> = options.get("mutable_dirs_shared")
                                                    .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                        if let Some(index) = dir.find("::") {
                                                            vec![&dir[..index], &dir[index + 2..]]
                                                        } else {
                                                            vec![dir]
                                                        }
                                                    }).filter(|dir| !dir.trim().is_empty()).collect()})
                                                    .unwrap_or_else(|| Vec::new());
        if !mutable_dirs.is_empty() {
            // Clean old mutable_dirs
            if Path::new("/.snapshots/mutable_dirs/snapshot-0").try_exists()? {
                remove_dir_content("/.snapshots/mutable_dirs/snapshot-0")?;
            }
            for mount_path in mutable_dirs {
                if allow_dir_mut(mount_path) {
                    // Create mouth_path directory in snapshot
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/mutable_dirs/snapshot-0/{}", mount_path))?;
                    // Create mouth_path directory in snapshot-chr
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/rootfs/snapshot-chr0/{}", mount_path))?;
                    // Use mount_path
                    #[cfg(feature = "btrfs")]
                    mount(Some(format!("/.snapshots/mutable_dirs/snapshot-0/{}", mount_path).as_str()),
                          format!("/.snapshots/rootfs/snapshot-chr0/{}", mount_path).as_str(),
                          Some("btrfs"), MsFlags::MS_BIND , None::<&str>)?;
                }
            }
        }
        if !mutable_dirs_shared.is_empty() {
            // Never wipe the shared store: it is persistent data bind-mounted
            // into the deployed system (e.g. /var) and shared by every
            // snapshot. Only ensure the configured subdirectories exist
            for mount_path in mutable_dirs_shared {
                if allow_dir_mut(mount_path) {
                    // Create mouth_path directory in snapshot
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/mutable_dirs/{}", mount_path))?;
                    // Create mouth_path directory in snapshot-chr
                    DirBuilder::new().recursive(true)
                                     .create(format!("/.snapshots/rootfs/snapshot-chr0/{}", mount_path))?;
                    // Use mount_path
                    #[cfg(feature = "btrfs")]
                    mount(Some(format!("/.snapshots/mutable_dirs/{}", mount_path).as_str()),
                          format!("/.snapshots/rootfs/snapshot-chr0/{}", mount_path).as_str(),
                          Some("btrfs"), MsFlags::MS_BIND , None::<&str>)?;
                }
            }
        }

        // File operations for snapshot-chr
        create_snapshot("/.snapshots/boot/boot-0",
                        "/.snapshots/boot/boot-chr0",
                        false)?;
        create_snapshot("/.snapshots/etc/etc-0",
                        "/.snapshots/etc/etc-chr0",
                        false)?;
        create_snapshot("/.snapshots/var/var-0",
                        "/.snapshots/var/var-chr0",
                        false)?;

        // Copy dsnap related configurations
        if Path::new("/etc/systemd").try_exists()? {
            // Machine-id is a Systemd thing
            copy("/etc/machine-id", "/.snapshots/rootfs/snapshot-chr0/etc/machine-id")?;
        }
        DirBuilder::new().recursive(true)
                         .create("/.snapshots/rootfs/snapshot-chr0/.snapshots/dsnap")?;
        copy("/.snapshots/dsnap/fstree", "/.snapshots/rootfs/snapshot-chr0/.snapshots/dsnap/fstree")?;

        // Check if dsnap exists
        if !Path::new("/.snapshots/rootfs/snapshot-chr0/usr/sbin/dsnap").try_exists()? {
            return Err(Error::new(ErrorKind::NotFound,
                                  "/usr/sbin/dsnap doesn't exist."));
        }

        Ok(snapshot)
    } else {
        return Err(Error::new(ErrorKind::Interrupted,
                              "Aborted."));
    }
}

// Install packages
pub fn install(snapshot: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot install as snapshot {} doesn't exist.", snapshot)));

        // Make sure snapshot is not in use by another dsnap process
        } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(
            Error::new(ErrorKind::Unsupported,
                       format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                               snapshot,snapshot)));

        // Make sure snapshot is not base snapshot
        } else if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Changing base snapshot is not allowed.")));

        // Install package
        } else {
        if let Err(e) = install_package_helper(snapshot, &pkgs, noconfirm) {
            let _ = chr_delete(snapshot);
            return Err(Error::new(ErrorKind::Interrupted,
                                  format!("Install failed and changes discarded: {}", e)));
        }
        if let Err(e) = post_transactions(snapshot) {
            let _ = chr_delete(snapshot);
            return Err(Error::new(ErrorKind::Other,
                                  format!("Install failed and changes discarded: {}", e)));
        }
    }
    Ok(())
}

// Install live
pub fn install_live(pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    let snapshot = get_current_snapshot()?;
    let tmp = get_tmp();
    dsnap_mounts(&tmp, "").map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let run = install_package_helper_live(&snapshot, &tmp, &pkgs, noconfirm);
    // Umount even when the install failed, otherwise the mounts leak
    if let Err(e) = dsnap_umounts(&tmp, "") {
        if run.is_ok() {
            return Err(Error::new(ErrorKind::Other, e.to_string()));
        }
        eprintln!("Failed to unmount live snapshot {}: {}", tmp, e);
    }
    run
}

// Upgrade the currently deployed snapshot in place, without reboot.
pub fn upgrade_live(snapshot: &str, noconfirm: bool) -> Result<(), Error> {
    let current = get_current_snapshot()?;
    if snapshot != current {
        return Err(Error::new(ErrorKind::Unsupported,
                              "Can't use the live option with any other snapshot than the current one.".to_string()));
    }

    println!("Upgrading the live snapshot in place; changes cannot be discarded. A kernel update still requires a reboot.");
    let tmp = get_tmp();
    dsnap_mounts(&tmp, "").map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let run = upgrade_helper_live(&tmp, noconfirm);
    // Umount even when the upgrade failed, otherwise the mounts leak
    if let Err(e) = dsnap_umounts(&tmp, "") {
        if run.is_ok() {
            return Err(Error::new(ErrorKind::Other, e.to_string()));
        }
        eprintln!("Failed to unmount live snapshot {}: {}", tmp, e);
    }
    run
}

// Install a profile from a text file
fn install_profile(snapshot: &str, profile: &str, force: bool, secondary: bool,
                   user_profile: &str, noconfirm: bool) -> Result<bool, Error> {
    // Get some values
    let distro = detect::distro_id();
    let dist_name = if distro.contains("_ashos") {
        distro.replace("_ashos", "")
    } else {
        distro
    };
    let cfile = format!("/usr/share/dsnap/profiles/{}/{}", profile,dist_name);

    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot install as snapshot {} doesn't exist.", snapshot)));

        // Make sure snapshot is not in use by another dsnap process
        } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(
            Error::new(ErrorKind::Unsupported,
                       format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                               snapshot,snapshot)));

        // Make sure snapshot is not base snapshot
        } else if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Changing base snapshot is not allowed.")));
    } else {
        // Install profile
        if user_profile.is_empty() {
            println!("Updating the system before installing {} profile...", profile);
        } else {
            println!("Updating the system before installing {} profile...", user_profile);
        }
        // Prepare
        auto_upgrade(snapshot)?;
        prepare(snapshot)?;
        let run = (|| -> Result<bool, Error> {
        // Profile configurations
        let mut profconf = Ini::new_cs();
        profconf.set_comment_symbols(&['#']);
        profconf.set_multiline(true);
        // Load profile if exist
        if Path::new(&cfile).is_file() && !force && user_profile.is_empty() {
            profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        } else if force {
            println!("Installing profiles...");
            install_package_helper(snapshot, &vec!["distro-snapper-profiles".to_string()], true)?;
            // The package places the profiles inside the snapshot chroot,
            // not on the host; load the profile from there
            let chr_cfile = format!("/.snapshots/rootfs/snapshot-chr{}/usr/share/dsnap/profiles/{}/{}", snapshot, profile, dist_name);
            profconf.load(&chr_cfile).map_err(|_| Error::new(ErrorKind::NotFound,
                                  format!("Profile {} was not found in the distro-snapper-profiles package.", profile)))?;
        } else if !user_profile.is_empty() {
            profconf.load(user_profile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        } else if !Path::new(&cfile).is_file() && !force {
            return Err(Error::new(ErrorKind::NotFound,
                                  "Please install distro-snapper-profiles package.".to_string()));
        }

        // Read presets section in configuration file
        #[cfg(feature = "pacman")]
        if profconf.sections().contains(&"presets".to_string()) {
            if !aur_check(snapshot)? {
                return Err(Error::new(ErrorKind::Unsupported,
                                      "Please enable AUR.".to_string()));
            }
        }

        // Read packages section in configuration file
        if profconf.sections().contains(&"profile-packages".to_string()) {
            let section = profconf.get_map_ref().get("profile-packages")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing profile-packages section."))?;
            let mut pkgs: Vec<String> = section.keys().cloned().collect();
            pkgs.retain(|p| !p.trim().is_empty());
            // Install package(s)
            install_package_helper(snapshot, &pkgs, noconfirm)?;
        }

        // Read enable services section in configuration file
        if profconf.sections().contains(&"enable-services".to_string()) {
            let section = profconf.get_map_ref().get("enable-services")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing enable-services section."))?;
            let mut services: Vec<String> = section.keys().cloned().collect();
            services.retain(|s| !s.trim().is_empty());
            // Enable service(s)
            service_enable(snapshot, &services, "chr")?;
        }

        // Read commands section in configuration file
        if profconf.sections().contains(&"install-commands".to_string()) {
            let section = profconf.get_map_ref().get("install-commands")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing install-commands section."))?;
            for cmd in section.keys() {
                if !cmd.trim().is_empty() {
                    chroot_exec(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot), cmd)?;
                }
            }
        }

        // Ignore empty file
        if !profconf.sections().contains(&"profile-packages".to_string()) &&
            !profconf.sections().contains(&"enable-services".to_string()) &&
            !profconf.sections().contains(&"install-commands".to_string()) &&
            !(cfg!(feature = "pacman") && profconf.sections().contains(&"presets".to_string()))
        {
            return Err(Error::new(ErrorKind::InvalidInput,
                                  "The profile is empty or invalid.".to_string()));
        }
        Ok(secondary)
        })();
        if run.is_err() {
            let _ = chr_delete(snapshot);
        }
        run
    }
}

// Install profile in live snapshot
fn install_profile_live(snapshot: &str,profile: &str, force: bool, user_profile: &str, noconfirm: bool) -> Result<(), Error> {
    // Get some values
    let distro = detect::distro_id();
    let dist_name = if distro.contains("_ashos") {
        distro.replace("_ashos", "")
    } else {
        distro
    };
    let cfile = format!("/usr/share/dsnap/profiles/{}/{}", profile,dist_name);
    let tmp = get_tmp();

    // Prepare
    if user_profile.is_empty() {
        println!("Updating the system before installing {} profile...", profile);
    } else {
        println!("Updating the system before installing {} profile...", user_profile);
    }
    // Mount tmp
    dsnap_mounts(&tmp, "").map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let run = (|| -> Result<(), Error> {
    // Upgrade
    if upgrade_helper_live(&tmp, noconfirm).is_ok() {

        // Profile configurations
        let mut profconf = Ini::new_cs();
        profconf.set_comment_symbols(&['#']);
        profconf.set_multiline(true);

        // Load profile if exist
        if Path::new(&cfile).is_file() && !force && user_profile.is_empty() {
            profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        } else if force {
            println!("Installing profiles...");
            install_package_helper_live(snapshot, &tmp, &vec!["distro-snapper-profiles".to_string()], true)?;
            // The package installs into the live snapshot, not on the
            // host; load the profile from there
            let live_cfile = format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/profiles/{}/{}", tmp, profile, dist_name);
            profconf.load(&live_cfile).map_err(|_| Error::new(ErrorKind::NotFound,
                                  format!("Profile {} was not found in the distro-snapper-profiles package.", profile)))?;
        } else if !user_profile.is_empty() {
            profconf.load(user_profile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        } else if !Path::new(&cfile).is_file() && !force {
            return Err(Error::new(ErrorKind::NotFound,
                                  "Please install distro-snapper-profiles package.".to_string()));
        }

        // Read presets section in configuration file
        #[cfg(feature = "pacman")]
        if profconf.sections().contains(&"presets".to_string()) {
            if !aur_check(snapshot)? {
                return Err(Error::new(ErrorKind::Unsupported,
                                      "Please enable AUR.".to_string()));
            }
        }

        // Read packages section in configuration file
        if profconf.sections().contains(&"profile-packages".to_string()) {
            let section = profconf.get_map_ref().get("profile-packages")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing profile-packages section."))?;
            let mut pkgs: Vec<String> = section.keys().cloned().collect();
            pkgs.retain(|p| !p.trim().is_empty());
            // Install package(s)
            install_package_helper_live(snapshot, &tmp, &pkgs, noconfirm)?;
        }

        // Read enable services section in configuration file
        if profconf.sections().contains(&"enable-services".to_string()) {
            let section = profconf.get_map_ref().get("enable-services")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing enable-services section."))?;
            let mut services: Vec<String> = section.keys().cloned().collect();
            services.retain(|s| !s.trim().is_empty());
            // Enable service(s) in the live snapshot
            service_enable(&tmp, &services, "")?;
        }

        // Read commands section in configuration file
        if profconf.sections().contains(&"install-commands".to_string()) {
            let section = profconf.get_map_ref().get("install-commands")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing install-commands section."))?;
            for cmd in section.keys() {
                if !cmd.trim().is_empty() {
                    chroot_exec(&format!("/.snapshots/rootfs/snapshot-{}", tmp), cmd)?;
                }
            }
        }

        // Ignore empty file
        if !profconf.sections().contains(&"profile-packages".to_string()) &&
            !profconf.sections().contains(&"enable-services".to_string()) &&
            !profconf.sections().contains(&"install-commands".to_string()) &&
            !(cfg!(feature = "pacman") && profconf.sections().contains(&"presets".to_string()))
        {
            return Err(Error::new(ErrorKind::InvalidInput,
                                  "The profile is empty or invalid.".to_string()));
        }

    } else {
        return Err(Error::new(ErrorKind::Interrupted,
                              "System update failed.".to_string()));
    }
    Ok(())
    })();

    // Umount tmp: the mounts must be released even when the profile
    // installation failed partway
    if let Err(e) = dsnap_umounts(&tmp, "") {
        if run.is_ok() {
            return Err(Error::new(ErrorKind::Other, e.to_string()));
        }
        eprintln!("Failed to unmount live snapshot {}: {}", tmp, e);
    }

    run
}

// Triage functions for argparse method
pub fn install_triage(snapshot: &str, live: bool, pkgs: Vec<String>, profile: &str, force: bool,
                      user_profile: &str, noconfirm: bool, secondary: bool) -> Result<(), Error> {
    // Switch between profile and user_profile
    let p = if user_profile.is_empty() {
        profile
    } else {
        user_profile
    };

    if !live {
        // Install profile
        if !profile.is_empty() {
            let secondary = install_profile(snapshot, profile, force, secondary, user_profile, noconfirm)?;
            if post_transactions(snapshot).is_ok() {
                println!("Profile {} installed in snapshot {} successfully.", p, snapshot);
                if yes_no(
                    &format!
                        ("Would you like to proceed with the deployment of snapshot {}?", snapshot)) {
                    if deploy(snapshot, secondary, false).is_ok() {
                        println!("Snapshot {} deployed to '/'.", snapshot);
                    }
                }
            } else {
                let _ = chr_delete(snapshot);
                return Err(Error::new(ErrorKind::Other,
                                      "Install failed and changes discarded.".to_string()));
            }

        } else if !pkgs.is_empty() {
            // Install package
            install(snapshot, &pkgs, noconfirm)?;
            println!("Package(s) {pkgs:?} installed in snapshot {} successfully.", snapshot);

        } else if !user_profile.is_empty() {
            // Install user_profile
            let secondary = install_profile(snapshot, profile, force, secondary, user_profile, noconfirm)?;
            if post_transactions(snapshot).is_ok() {
                println!("Profile {} installed in snapshot {} successfully.", p, snapshot);
                if yes_no(&format!("Would you like to proceed with the deployment of snapshot {}?", snapshot)) {
                    if deploy(snapshot, secondary, false).is_ok() {
                        println!("Snapshot {} deployed to '/'.", snapshot);
                    }
                }
            } else {
                let _ = chr_delete(snapshot);
                return Err(Error::new(ErrorKind::Other,
                                      "Install failed and changes discarded.".to_string()));
            }
        }

    } else if live && snapshot != &get_current_snapshot()? {
        // Prevent live option if snapshot is not current snapshot
        return Err(Error::new(ErrorKind::Unsupported,
                              "Can't use the live option with any other snapshot than the current one.".to_string()));

    // Do live install only if: live flag is used OR target snapshot is current
    } else if live && snapshot == &get_current_snapshot()? {
        if !profile.is_empty() {
            // Live profile installation
            install_profile_live(snapshot, profile, force, user_profile, noconfirm)?;
            println!("Profile {} installed in current/live snapshot.", p);

        } else if !pkgs.is_empty() {
            // Live package installation
            install_live(&pkgs, noconfirm)?;
            println!("Package(s) {pkgs:?} installed in snapshot {} successfully.", snapshot);

        } else if !user_profile.is_empty() {
            // Live user_profile installation
            install_profile_live(snapshot, profile, force, user_profile, noconfirm)?;
            println!("Profile {} installed in current/live snapshot.", p);
        }
    }
    Ok(())
}

// Check EFI
pub fn is_efi() -> bool {
    Path::new("/sys/firmware/efi").try_exists().unwrap_or(false)
}

// Check if path is mounted
fn is_mounted(path: &Path) -> Result<bool, Error> {
    let mount_iter = MountIter::new().map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    for mount in mount_iter {
        let mount = mount.map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        if mount.dest == path {
            return Ok(true);
        }
    }
    Ok(false)
}

// Check if path or anything under it is mounted
fn path_is_mounted_or_under(path: &Path) -> Result<bool, Error> {
    let mount_iter = MountIter::new().map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let p = path.as_os_str().to_string_lossy();
    let p = p.trim_end_matches('/');
    for mount in mount_iter {
        let mount = mount.map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        let d = mount.dest.as_os_str().to_string_lossy();
        let d = d.trim_end_matches('/');
        if d == p || d.starts_with(&format!("{}/", p)) {
            return Ok(true);
        }
    }
    Ok(false)
}

// Return if package installed in snapshot
pub fn is_pkg_installed(snapshot: &str, pkg: &str) -> Result<bool, Error> {
    Ok(pkg_list(snapshot, "")?.contains(&pkg.to_string()))
}

// Check if system packages is locked
fn is_system_locked() -> bool {
    if cfg!(feature = "lock") {
        return true;
    } else {
        return false;
    }
}

// Check if package in "system-packages" list
fn is_system_pkg(profconf: &Ini, pkg: String) -> bool {
    profconf.get_map()
            .and_then(|map| map.get("system-packages").cloned())
            .map(|section| section.contains_key(&pkg))
            .unwrap_or(false)
}

// Package list
pub fn list(snapshot: &str, chr: &str, exclude: bool) -> Result<Vec<String>, Error> {
    let list = if exclude {
        prepare(snapshot)?;
        let result = no_dep_pkg_list(snapshot, chr)?;
        post_transactions(snapshot)?;
        result
    } else {
        pkg_list(snapshot, chr)?
    };
    Ok(list)
}

// List sub-volumes for the booted distro only
pub fn list_subvolumes() -> Result<(), Error> {
    #[cfg(feature = "btrfs")]
    {
        let status = Command::new("sh").arg("-c")
            .arg("btrfs sub list / | grep -i _linux | sort -f -k 9")
            .status()?;
        if !status.success() {
            return Err(Error::new(ErrorKind::Other, "Failed to list subvolumes.".to_string()));
        }
    }
    #[cfg(not(feature = "btrfs"))]
    {
        return Err(Error::new(ErrorKind::Unsupported, "Subvolume listing is not supported for this filesystem.".to_string()));
    }
    Ok(())
}

// Enable or disable systemd unit(s) in a snapshot and keep the profile in sync.
pub fn service_control(snapshot: &str, enable: &Vec<String>, disable: &Vec<String>) -> Result<(), Error> {
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot change services as snapshot {} doesn't exist.", snapshot)));
    }
    if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                                      snapshot, snapshot)));
    }
    if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported,
                              "Changing services in base snapshot is not allowed.".to_string()));
    }
    if enable.is_empty() && disable.is_empty() {
        return Ok(());
    }

    prepare(snapshot)?;
    let run = (|| -> Result<(), Error> {
        if !enable.is_empty() {
            service_enable(snapshot, enable, "chr")?;
        }
        if !disable.is_empty() {
            service_disable(snapshot, disable, "chr")?;
        }

        let cfile = format!("/.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot);
        let mut profconf = Ini::new_cs();
        profconf.set_comment_symbols(&['#']);
        profconf.set_multiline(true);
        profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        let mut write_options = WriteOptions::default();
        write_options.blank_lines_between_sections = 1;

        for unit in enable {
            profconf.remove_key("enable-services", unit);
            profconf.set("enable-services", unit, None);
        }
        for unit in disable {
            profconf.remove_key("enable-services", unit);
        }
        profile_write(&cfile, &profconf, &write_options)
    })();
    if let Err(e) = run {
        let _ = chr_delete(snapshot);
        return Err(e);
    }
    if let Err(e) = post_transactions(snapshot) {
        let _ = chr_delete(snapshot);
        return Err(Error::new(ErrorKind::Other,
                              format!("Service change failed and changes discarded: {}", e)));
    }
    Ok(())
}

// Live unlocked shell
pub fn live_unlock() -> Result<(), Error> {
    let tmp = get_tmp();
    dsnap_mounts(&tmp, "")?;
    let run = chroot_in(&format!("/.snapshots/rootfs/snapshot-{}", tmp));
    // Umount even when the shell could not be started, otherwise the
    // mounts leak
    dsnap_umounts(&tmp, "")?;
    run.map(|_| ())
}

// Auto update
pub fn noninteractive_update(snapshot: &str) -> Result<(), Error> {
    auto_upgrade(snapshot)
}

// Post transaction function, copy from chroot directories back to read only snapshot directories
pub fn post_transactions(snapshot: &str) -> Result<(), Error> {
    //File operations in snapshot-chr
    remove_dir_content(&format!("/.snapshots/boot/boot-chr{}", snapshot))?;
        cp_reflink(format!("/.snapshots/rootfs/snapshot-chr{}/boot/.", snapshot),
               format!("/.snapshots/boot/boot-chr{}", snapshot))?;
    remove_dir_content(&format!("/.snapshots/etc/etc-chr{}", snapshot))?;
        cp_reflink(format!("/.snapshots/rootfs/snapshot-chr{}/etc/.", snapshot),
               format!("/.snapshots/etc/etc-chr{}", snapshot))?;
    // Keep package manager's cache after installing packages
    // This prevents unnecessary downloads for each snapshot when upgrading multiple snapshots
    cache_copy(snapshot, false)?;
    // Clean cache for base snapshot
    if snapshot == "0" {
        clean_cache(snapshot)?;
    }
    remove_dir_content(&format!("/.snapshots/var/var-chr{}", snapshot))?;
        cp_reflink(format!("/.snapshots/rootfs/snapshot-chr{}/var/.", snapshot),
               format!("/.snapshots/var/var-chr{}", snapshot))?;

    // Delete old snapshot
    let path_vec = vec!["boot","etc", "var"];
    for path in path_vec {
        if Path::new(&format!("/.snapshots/{}/{}-{}", path,path,snapshot)).try_exists()? {
            delete_subvolume(&format!("/.snapshots/{}/{}-{}", path,path,snapshot))?;
        }
    }
    if Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        delete_subvolume(&format!("/.snapshots/rootfs/snapshot-{}", snapshot))?;
    }

    // Create mutable or immutable snapshot
    // Mutable
    if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}/usr/share/dsnap/mutable", snapshot)).try_exists()? {
        create_snapshot(&format!("/.snapshots/boot/boot-chr{}", snapshot),
                        &format!("/.snapshots/boot/boot-{}", snapshot),
                        false)?;
        create_snapshot(&format!("/.snapshots/etc/etc-chr{}", snapshot),
                        &format!("/.snapshots/etc/etc-{}", snapshot),
                        false)?;
        create_snapshot(&format!("/.snapshots/var/var-chr{}", snapshot),
                        &format!("/.snapshots/var/var-{}", snapshot),
                        false)?;
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot),
                        &format!("/.snapshots/rootfs/snapshot-{}", snapshot),
                        false)?;
    } else {
        // Immutable
        create_snapshot(&format!("/.snapshots/boot/boot-chr{}", snapshot),
                        &format!("/.snapshots/boot/boot-{}", snapshot),
                        true)?;
        create_snapshot(&format!("/.snapshots/etc/etc-chr{}", snapshot),
                        &format!("/.snapshots/etc/etc-{}", snapshot),
                        true)?;
        create_snapshot(&format!("/.snapshots/var/var-chr{}", snapshot),
                        &format!("/.snapshots/var/var-{}", snapshot),
                        true)?;
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot),
                        &format!("/.snapshots/rootfs/snapshot-{}", snapshot),
                        true)?;
    }

    // Umount in reverse order
    dsnap_umounts(snapshot, "chr")?;

    // Special mutable directories
    let config = snapshot_config(snapshot)?;

    for mount_path in &config.mutable_dirs {
        if !allow_dir_mut(mount_path) {
            return Err(Error::new(ErrorKind::InvalidInput,
                                  format!("Please insert valid value for mutable_dirs in /.snapshots/etc/etc-{}/dsnap/dsnap.conf", snapshot)));
        }
        let mount = format!("/.snapshots/rootfs/snapshot-chr{}/{}", snapshot, mount_path);
        if is_mounted(Path::new(&mount))? {
            umount2(Path::new(&mount), MntFlags::MNT_DETACH)?;
        }
    }
    for mount_path in &config.mutable_dirs_shared {
        if !allow_dir_mut(mount_path) {
            return Err(Error::new(ErrorKind::InvalidInput,
                                  format!("Please insert valid value for mutable_dirs_shared in /.snapshots/etc/etc-{}/dsnap/dsnap.conf", snapshot)));
        }
        let mount = format!("/.snapshots/rootfs/snapshot-chr{}/{}", snapshot, mount_path);
        if is_mounted(Path::new(&mount))? {
            umount2(Path::new(&mount), MntFlags::MNT_DETACH)?;
        }
    }

    // Clean chroot
    chr_delete(snapshot)?;

    Ok(())
}

// Tear down mounts created by prepare() and hollow() without committing content
fn hollow_teardown(snapshot: &str) -> Result<(), Error> {
    // Umount the nested chroot view inside the live-root bind
    let nested = format!("/.snapshots/rootfs/snapshot-chr{}/.snapshots/rootfs/snapshot-chr{}/", snapshot, snapshot);
    if is_mounted(Path::new(&nested))? {
        umount2(Path::new(&nested), MntFlags::MNT_DETACH)?;
    }
    // Umount the live root bind hollow() put on top of the chroot
    let chr_path = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    if is_mounted(Path::new(&chr_path))? {
        umount2(Path::new(&chr_path), MntFlags::MNT_DETACH)?;
    }
    // Unwind the chroot mounts prepare() created (resolv, sys, proc, ..., self-bind)
    dsnap_umounts(snapshot, "chr")?;
    // Umount the mutable_dirs binds prepare() mounted into the chroot;
    // dsnap_umounts() does not know about them
    let options = snapshot_config_get(snapshot);
    let mut mount_paths: Vec<String> = Vec::new();
    for key in ["mutable_dirs", "mutable_dirs_shared"] {
        if let Some(dirs) = options.get(key) {
            for dir in dirs.split(',') {
                for part in dir.split("::") {
                    let part = part.trim();
                    if !part.is_empty() {
                        mount_paths.push(part.to_string());
                    }
                }
            }
        }
    }
    for mount_path in mount_paths {
        let mount = format!("/.snapshots/rootfs/snapshot-chr{}/{}", snapshot, mount_path);
        if is_mounted(Path::new(&mount))? {
            umount2(Path::new(&mount), MntFlags::MNT_DETACH)?;
        }
    }
    Ok(())
}

// Hollow  Post transaction function
pub fn posttrans(snapshot: &str) -> Result<(), Error> {
    delete_subvolume(&format!("/.snapshots/rootfs/snapshot-{}", snapshot))?;
    remove_dir_content(&format!("/.snapshots/etc/etc-chr{}", snapshot))?;
        cp_reflink(format!("/.snapshots/rootfs/snapshot-chr{}/etc/.", snapshot),
               format!("/.snapshots/etc/etc-chr{}", snapshot))?;
    remove_dir_content(&format!("/.snapshots/var/var-chr{}", snapshot))?;
        cp_reflink(format!("/.snapshots/rootfs/snapshot-chr{}/var/.", snapshot),
               format!("/.snapshots/var/var-chr{}", snapshot))?;
    remove_dir_content(&format!("/.snapshots/boot/boot-chr{}",  snapshot))?;
        cp_reflink(format!("/.snapshots/rootfs/snapshot-chr{}/boot/.", snapshot),
               format!("/.snapshots/boot/boot-chr{}", snapshot))?;
    delete_subvolume(&format!("/.snapshots/etc/etc-{}", snapshot))?;
    delete_subvolume(&format!("/.snapshots/var/var-{}", snapshot))?;
    delete_subvolume(&format!("/.snapshots/boot/boot-{}", snapshot))?;
    create_snapshot(&format!("/.snapshots/etc/etc-chr{}", snapshot),
                    &format!("/.snapshots/etc/etc-{}", snapshot),
                    true)?;
    create_snapshot(&format!("/.snapshots/var/var-chr{}", snapshot),
                    &format!("/.snapshots/var/var-{}", snapshot),
                    true)?;
    create_snapshot(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot),
                    &format!("/.snapshots/rootfs/snapshot-{}", snapshot),
                    true)?;
    create_snapshot(&format!("/.snapshots/boot/boot-chr{}", snapshot),
                    &format!("/.snapshots/boot/boot-{}", snapshot),
                    true)?;
    // Tear down every mount before deleting the chroot. delete_subvolume
    // fails with EBUSY while anything is still mounted, and chr_delete's
    // fallback would then walk the topmost mount, which is the recursive
    // bind of the live root filesystem hollow() placed here, deleting the
    // running system's files through it.
    hollow_teardown(snapshot)?;
    chr_delete(snapshot)?;
    Ok(())
}

// Prepare snapshot to chroot directory to install or chroot into
pub fn prepare(snapshot: &str) -> Result<(), Error> {
    chr_delete(snapshot)?;
    let snapshot_chr = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);

    // Create chroot directory
    create_snapshot(&format!("/.snapshots/rootfs/snapshot-{}", snapshot),
                    &snapshot_chr,
                    false)?;

    // Pacman gets weird when chroot directory is not a mountpoint, so the following mount is necessary
    dsnap_mounts(snapshot, "chr")?;

    // Special mutable directories
    let options = snapshot_config_get(snapshot);
    let mutable_dirs: Vec<&str> = options.get("mutable_dirs")
                                                .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                    if let Some(index) = dir.find("::") {
                                                        vec![&dir[..index], &dir[index + 2..]]
                                                    } else {
                                                        vec![dir]
                                                    }
                                                }).filter(|dir| !dir.trim().is_empty()).collect()})
                                                .unwrap_or_else(|| Vec::new());
    let mutable_dirs_shared: Vec<&str> = options.get("mutable_dirs_shared")
                                                .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                    if let Some(index) = dir.find("::") {
                                                        vec![&dir[..index], &dir[index + 2..]]
                                                    } else {
                                                        vec![dir]
                                                    }
                                                }).filter(|dir| !dir.trim().is_empty()).collect()})
                                                .unwrap_or_else(|| Vec::new());

    if !mutable_dirs.is_empty() {
        // Clean old mutable_dirs
        if Path::new(&format!("/.snapshots/mutable_dirs/snapshot-{}", snapshot)).try_exists()? {
            remove_dir_content(&format!("/.snapshots/mutable_dirs/snapshot-{}", snapshot))?;
        }
        for mount_path in mutable_dirs {
            if allow_dir_mut(mount_path) {
                // Create mouth_path directory in snapshot
                DirBuilder::new().recursive(true)
                                 .create(format!("/.snapshots/mutable_dirs/snapshot-{}/{}", snapshot,mount_path))?;
                // Create mouth_path directory in snapshot-chr
                DirBuilder::new().recursive(true)
                                 .create(format!("{}/{}", snapshot_chr,mount_path))?;
                // Use mount_path
                #[cfg(feature = "btrfs")]
                mount(Some(format!("/.snapshots/mutable_dirs/snapshot-{}/{}", snapshot,mount_path).as_str()),
                      format!("{}/{}", snapshot_chr,mount_path).as_str(),
                      Some("btrfs"), MsFlags::MS_BIND , None::<&str>)?;
            }
        }
    }
    if !mutable_dirs_shared.is_empty() {
        // Never wipe the shared store: it is persistent data bind-mounted
        // into the deployed system (e.g. /var) and shared by every
        // snapshot. Only ensure the configured subdirectories exist
        for mount_path in mutable_dirs_shared {
            if allow_dir_mut(mount_path) {
                // Create mouth_path directory in snapshot
                DirBuilder::new().recursive(true)
                                 .create(format!("/.snapshots/mutable_dirs/{}", mount_path))?;
                // Create mouth_path directory in snapshot-chr
                DirBuilder::new().recursive(true)
                                 .create(format!("{}/{}", snapshot_chr,mount_path))?;
                // Use mount_path
                #[cfg(feature = "btrfs")]
                mount(Some(format!("/.snapshots/mutable_dirs/{}", mount_path).as_str()),
                      format!("{}/{}", snapshot_chr,mount_path).as_str(),
                      Some("btrfs"), MsFlags::MS_BIND , None::<&str>)?;
            }
        }
    }

    // File operations for snapshot-chr
    create_snapshot(&format!("/.snapshots/boot/boot-{}", snapshot),
                    &format!("/.snapshots/boot/boot-chr{}", snapshot),
                    false)?;
    create_snapshot(&format!("/.snapshots/etc/etc-{}", snapshot),
                    &format!("/.snapshots/etc/etc-chr{}", snapshot),
                    false)?;
    create_snapshot(&format!("/.snapshots/var/var-{}", snapshot),
                    &format!("/.snapshots/var/var-chr{}", snapshot),
                    false)?;
        cp_reflink(format!("/.snapshots/boot/boot-chr{}/.", snapshot),
               format!("{}/boot", snapshot_chr))?;
        cp_reflink(format!("/.snapshots/etc/etc-chr{}/.", snapshot),
               format!("{}/etc", snapshot_chr))?;
        cp_reflink(format!("/.snapshots/var/var-chr{}/.", snapshot),
               format!("{}/var", snapshot_chr))?;

    // Copy dsnap related configurations
    if Path::new("/etc/systemd").try_exists()? {
        // Machine-id is a Systemd thing
        copy("/etc/machine-id", format!("{}/etc/machine-id", snapshot_chr))?;
    }
    DirBuilder::new().recursive(true)
                     .create(format!("{}/.snapshots/dsnap", snapshot_chr))?;
    copy("/.snapshots/dsnap/fstree", format!("{}/.snapshots/dsnap/fstree", snapshot_chr))?;

    // Keep package manager's cache after installing packages
    // This prevents unnecessary downloads for each snapshot when upgrading multiple snapshots
    if snapshot != "0" {
        cache_copy(snapshot, true)?;
    }

    Ok(())
}

// Show tmp partition state
pub fn print_tmp() -> Result<String, Error> {
    // By default just return which deployment is running
    let mounts = read_to_string("/proc/mounts")?;
    let mount: Vec<&str> = mounts
        .lines()
        .filter(|line| {
            #[cfg(feature = "btrfs")]
            { line.contains(" / btrfs") }
            #[cfg(not(feature = "btrfs"))]
            { let _ = line; false }
        })
        .collect();
    if mount.iter().any(|element| element.contains("recovery-deploy-aux")) {
        Ok("recovery-deploy-aux".to_string())
    } else if mount.iter().any(|element| element.contains("recovery-deploy")) {
        Ok("recovery-deploy".to_string())
    } else if mount.iter().any(|element| element.contains("deploy-aux-secondary")) {
        Ok("deploy-aux-secondary".to_string())
    } else if mount.iter().any(|element| element.contains("deploy-secondary")) {
        Ok("deploy-secondary".to_string())
    } else if mount.iter().any(|element| element.contains("deploy-aux")) {
        Ok("deploy-aux".to_string())
    } else {
        Ok("deploy".to_string())
    }
}

// Rebuild snapshot
pub fn rebuild(snapshot: &str, snap_num: i32, desc: &str) -> Result<i32, Error> {
    // Make sure snapshot does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound, format!("Cannot rebuild as snapshot {} doesn't exist.", snapshot)));

    // Make sure snapshot is not in use by another dsnap process
    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(
            Error::new(
                ErrorKind::Unsupported,
                format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                        snapshot,snapshot)));

    // Make sure snap_num is not in use by another dsnap process
    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snap_num)).try_exists()? {
        return Err(
            Error::new(
                ErrorKind::Unsupported,
                format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                        snap_num,snap_num)));

    // Make sure snapshot is not base snapshot
    } else if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported, "Changing base snapshot is not allowed."));

    } else {
        // Make snapshot mutable or immutable
        let immutability = if check_mutability(snapshot) {
            false
        } else {
            true
        };

        // prepare
        rebuild_prep(snapshot)?;

        // Create snapshot
        create_snapshot(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot),
                        &format!("/.snapshots/rootfs/snapshot-chr{}", snap_num),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/boot/boot-chr{}", snapshot),
                        &format!("/.snapshots/boot/boot-chr{}", snap_num),
                        immutability)?;
        create_snapshot(&format!("/.snapshots/etc/etc-chr{}", snapshot),
                        &format!("/.snapshots/etc/etc-chr{}", snap_num),
                        immutability)?;
        // Keep package manager's cache after installing packages
        // This prevents unnecessary downloads for each snapshot when upgrading multiple snapshots
        cache_copy(snapshot, false)?;
        create_snapshot(&format!("/.snapshots/var/var-chr{}", snapshot),
                        &format!("/.snapshots/var/var-chr{}", snap_num),
                        immutability)?;

        // Umount in reverse order
        dsnap_umounts(snapshot, "chr")?;

        // Delete old snapshot chroot
        chr_delete(snapshot)?;

        // Create mutable or immutable snapshot
        // Mutable
        if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}/usr/share/dsnap/mutable", snap_num)).try_exists()? {
            create_snapshot(&format!("/.snapshots/boot/boot-chr{}", snap_num),
                            &format!("/.snapshots/boot/boot-{}", snap_num),
                            false)?;
            create_snapshot(&format!("/.snapshots/etc/etc-chr{}", snap_num),
                            &format!("/.snapshots/etc/etc-{}", snap_num),
                            false)?;
            create_snapshot(&format!("/.snapshots/var/var-chr{}", snap_num),
                            &format!("/.snapshots/var/var-{}", snap_num),
                            false)?;
            create_snapshot(&format!("/.snapshots/rootfs/snapshot-chr{}", snap_num),
                            &format!("/.snapshots/rootfs/snapshot-{}", snap_num),
                            false)?;
        } else {
            // Immutable
            create_snapshot(&format!("/.snapshots/boot/boot-chr{}", snap_num),
                            &format!("/.snapshots/boot/boot-{}", snap_num),
                            true)?;
            create_snapshot(&format!("/.snapshots/etc/etc-chr{}", snap_num),
                            &format!("/.snapshots/etc/etc-{}", snap_num),
                            true)?;
            create_snapshot(&format!("/.snapshots/var/var-chr{}", snap_num),
                            &format!("/.snapshots/var/var-{}", snap_num),
                            true)?;
            create_snapshot(&format!("/.snapshots/rootfs/snapshot-chr{}", snap_num),
                            &format!("/.snapshots/rootfs/snapshot-{}", snap_num),
                            true)?;
        }

        // Special mutable directories
        let options = snapshot_config_get(&format!("{}", snap_num));
        let mutable_dirs: Vec<&str> = options.get("mutable_dirs")
                                             .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                 if let Some(index) = dir.find("::") {
                                                     vec![&dir[..index], &dir[index + 2..]]
                                                 } else {
                                                     vec![dir]
                                                 }
                                             }).filter(|dir| !dir.trim().is_empty()).collect()})
                                             .unwrap_or_else(|| Vec::new());
        let mutable_dirs_shared: Vec<&str> = options.get("mutable_dirs_shared")
                                                    .map(|dirs| {dirs.split(',').flat_map(|dir| {
                                                        if let Some(index) = dir.find("::") {
                                                            vec![&dir[..index], &dir[index + 2..]]
                                                        } else {
                                                            vec![dir]
                                                        }
                                                    }).filter(|dir| !dir.trim().is_empty()).collect()})
                                                    .unwrap_or_else(|| Vec::new());

        if !mutable_dirs.is_empty() {
            for mount_path in mutable_dirs {
                if !allow_dir_mut(mount_path) {
                    return Err(Error::new(ErrorKind::InvalidInput,
                                          format!("Please insert valid value for mutable_dirs in /.snapshots/etc/etc-{}/dsnap/dsnap.conf", snapshot)));
                }
                let mount = format!("/.snapshots/rootfs/snapshot-chr{}/{}", snap_num, mount_path);
                if is_mounted(Path::new(&mount))? {
                    umount2(Path::new(&mount), MntFlags::MNT_DETACH)?;
                }
            }
        }
        if !mutable_dirs_shared.is_empty() {
            for mount_path in mutable_dirs_shared {
                if !allow_dir_mut(mount_path) {
                    return Err(Error::new(ErrorKind::InvalidInput,
                                          format!("Please insert valid value for mutable_dirs_shared in /.snapshots/etc/etc-{}/dsnap/dsnap.conf", snapshot)));
                }
                let mount = format!("/.snapshots/rootfs/snapshot-chr{}/{}", snap_num, mount_path);
                if is_mounted(Path::new(&mount))? {
                    umount2(Path::new(&mount), MntFlags::MNT_DETACH)?;
                }
            }
        }

        // Clean chroot
        chr_delete(&format!("{}", snap_num))?;

        // Mark newly created snapshot as mutable
        if !immutability {
            File::create(format!("/.snapshots/rootfs/snapshot-{}/usr/share/dsnap/mutable", snap_num))?;
        }

        // Import tree file
        let tree = fstree()?;
        // Add to root tree
        append_base_tree(&tree, snap_num)?;
        // Save tree to fstree
        write_tree(&tree)?;
        // Write description for snapshot
        if desc.is_empty() {
            let description = format!("rebuild of {}.", snapshot);
            write_desc(&snap_num.to_string(), &description, true)?;
        } else {
            write_desc(&snap_num.to_string(), &desc, true)?;
        }
    }
    Ok(snap_num)
}

// Rebuild base snapshot
pub fn rebuild_base() -> Result<(), Error> {
    // Make sure snapshot is not in use by another dsnap process
    if Path::new("/.snapshots/rootfs/snapshot-chr0").try_exists()? {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "Snapshot 0 appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s 0'."));
    } else {
        let snapshot = "0";
        if rebuild_prep(snapshot).is_ok() {
            post_transactions(snapshot)?;
        } else {
            chr_delete(snapshot)?;
            return Err(Error::new(
                ErrorKind::Other,
                "Rebuild failed and changes discarded."));
        }
    }
    Ok(())
}

// Prepare rebuild process
pub fn rebuild_prep(snapshot: &str) -> Result<(), Error> {
    //Profile configurations
    let cfile = format!("/.snapshots/etc/etc-{}/dsnap/profile", snapshot);
    let mut profconf = Ini::new_cs();
    profconf.set_comment_symbols(&['#']);
    profconf.set_multiline(true);
    // Load profile
    profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;

    // Create chroot directory
    let snapshot_chr = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    create_subvolume(&snapshot_chr)?;

    // Bootstrap snapshot
    bootstrap(snapshot)?;

    // Pacman gets weird when chroot directory is not a mountpoint, so the following mount is necessary
    dsnap_mounts(snapshot, "chr")?;

    // Copy old snapshot
    system_config(snapshot, &profconf)?;

    // File operations for snapshot-chr
    create_snapshot(&format!("/.snapshots/boot/boot-{}", snapshot),
                    &format!("/.snapshots/boot/boot-chr{}", snapshot),
                    false)?;
    create_snapshot(&format!("/.snapshots/etc/etc-{}", snapshot),
                    &format!("/.snapshots/etc/etc-chr{}", snapshot),
                    false)?;
    create_snapshot(&format!("/.snapshots/var/var-{}", snapshot),
                    &format!("/.snapshots/var/var-chr{}", snapshot),
                    false)?;
    remove_dir_content(&format!("/.snapshots/boot/boot-chr{}", snapshot))?;
        cp_reflink(format!("{}/boot/.", snapshot_chr),
               format!("/.snapshots/boot/boot-chr{}", snapshot))?;
    remove_dir_content(&format!("/.snapshots/etc/etc-chr{}", snapshot))?;
        cp_reflink(format!("{}/etc/.", snapshot_chr),
               format!("/.snapshots/etc/etc-chr{}", snapshot))?;
    remove_dir_content(&format!("/.snapshots/var/var-chr{}", snapshot))?;
        cp_reflink(format!("{}/var/.", snapshot_chr),
               format!("/.snapshots/var/var-chr{}", snapshot))?;

    Ok(())
}

// Refresh snapshot
pub fn refresh(snapshot: &str) -> Result<(), Error> {
    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot refresh as snapshot {} doesn't exist.", snapshot)));

        // Make sure snapshot is not in use by another dsnap process
    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                                      snapshot, snapshot)));

        // Make sure snapshot is not base snapshot
    } else if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported,
                              "Changing base snapshot is not allowed.".to_string()));
    }

    sync_time()?;
    prepare(snapshot)?;
    let excode = refresh_helper(snapshot);
    if excode.is_ok() {
        post_transactions(snapshot)?;
        println!("Snapshot {} refreshed successfully.", snapshot);
        Ok(())
    } else {
        chr_delete(snapshot)?;
        Err(Error::new(ErrorKind::Other,
                       "Refresh failed and changes discarded.".to_string()))
    }
}

// Remove directory contents
pub fn remove_dir_content(dir_path: &str) -> Result<(), Error> {
    // Specify the path to the directory to remove contents from
    let path = PathBuf::from(dir_path);

    // Iterate over the directory entries using the read_dir function
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();

        // Check if the entry is a file or a directory
        if path.is_file() {
            // If it's a file, remove it using the remove_file function
            std::fs::remove_file(path)?;
        } else if path.is_symlink() {
            // If it's a symlink, remove it using the remove_file function
            std::fs::remove_file(path)?;
        } else if path.is_dir() {
            // If it's a directory, recursively remove its contents using the remove_dir_all function
            std::fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}

// Copy recursively, using reflinks when supported, and make sure the copy
// actually succeeded: cp fails partially, so only its exit status tells
// whether every file made it to the destination.
pub fn cp_reflink(src: impl AsRef<str>, dst: impl AsRef<str>) -> Result<(), Error> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    let output = Command::new("cp").args(["-r", "--reflink=auto"])
                                   .arg(src)
                                   .arg(dst)
                                   .output()?;
    if !output.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to copy {} to {}: {}",
                                      src, dst,
                                      String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(())
}

// Copy recursively without clobbering existing files, using reflinks when
// supported, and make sure the copy actually succeeded.
pub fn cp_reflink_no_clobber(src: impl AsRef<str>, dst: impl AsRef<str>) -> Result<(), Error> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    let output = Command::new("cp").args(["-n", "-r", "--reflink=auto"])
                                   .arg(src)
                                   .arg(dst)
                                   .output()?;
    if !output.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to copy {} to {}: {}",
                                      src, dst,
                                      String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(())
}

// System reset
pub fn reset() -> Result<(), Error> {
    let current_snapshot = get_current_snapshot()?;
    let msg = "All snapshots will be permanently deleted and cannot be retrieved, are you absolutely certain you want to continue?";
    let reset_msg = "The system will restart automatically to complete the reset. Do you want to continue?";
    if yes_no(msg) && yes_no(reset_msg) {
        // Collect snapshots
        let tree = fstree()?;
        let snapshots = return_children(&tree, "root")?;

        // Prepare rc.local
        prepare("0")?;
        copy("/.snapshots/rootfs/snapshot-chr0/etc/rc.local", "/.snapshots/rootfs/snapshot-chr0/etc/rc.local.bak")?;
        let start = "#!/bin/sh";
        let del_snap = format!("/usr/sbin/dsnap del -q -n -s {}", current_snapshot);
        let cp_rc = "cp /etc/rc.local.bak /etc/rc.local";
        let mut file = OpenOptions::new().truncate(true)
                                         .read(true)
                                         .write(true)
                                         .open("/.snapshots/rootfs/snapshot-chr0/etc/rc.local")?;
        let new_content = format!("{}\n{}\n{}\nexit 0", start,del_snap,cp_rc);
        file.write_all(new_content.as_bytes())?;
        post_transactions("0")?;

        // Deploy the base snapshot and remove all the other snapshots
        if deploy("0", false, true).is_ok() {
            // Revers rc.local
            prepare("0")?;
            copy("/.snapshots/rootfs/snapshot-chr0/etc/rc.local.bak", "/.snapshots/rootfs/snapshot-chr0/etc/rc.local")?;
            post_transactions("0")?;
            for snapshot in snapshots {
                // Delete snapshot if exist
                if Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()?
                && snapshot.to_string() != current_snapshot && snapshot.to_string() != "0" {
                    delete_node(&vec![snapshot.to_string()], true, true)?;
                }
            }
        } else {
            return Err(Error::new(ErrorKind::Other,
                                  "Failed to deploy base snapshot."));
        }
    } else {
        return Err(Error::new(ErrorKind::Interrupted,
                              "Aborted."));
    }
    Ok(())
}

// Rollback last booted deployment //REVIEW => display manager on debian is issue
pub fn rollback() -> Result<(), Error> {
    let tmp = "rollback";
    let i = find_new()?;
    clone_as_tree(&tmp, "Rollback")?;
    deploy(&i.to_string(), false, false)?;
    Ok(())
}

// Creates new tree from base file
pub fn snapshot_base_new(desc: &str) -> Result<i32, Error> {
    // Immutability toggle not used as base should always be immutable
    let i = find_new()?;
    create_snapshot("/.snapshots/boot/boot-0",
                    &format!("/.snapshots/boot/boot-{}", i),
                    true)?;
    create_snapshot("/.snapshots/etc/etc-0",
                    &format!("/.snapshots/etc/etc-{}", i),
                    true)?;
    create_snapshot("/.snapshots/var/var-0",
                    &format!("/.snapshots/var/var-{}", i),
                    true)?;
    create_snapshot("/.snapshots/rootfs/snapshot-0",
                    &format!("/.snapshots/rootfs/snapshot-{}", i),
                    true)?;

    // Import tree file
    let tree = fstree()?;

    // Add to root tree
    append_base_tree(&tree, i)?;
    // Save tree to fstree
    write_tree(&tree)?;
    // Write description
    if desc.is_empty() {
        write_desc(&i.to_string(), "clone of base.", true)?;
    } else {
        write_desc(&i.to_string(), desc, true)?;
    }
    Ok(i)
}

// configparser discards comments when it loads an INI file. Capture the
// comments from the file on disk and re-attach them to their section or key
// after the INI has been re-serialized, so profile edits never lose them.
fn restore_profile_comments(old: &str, new: &str) -> String {
    let mut before_section: HashMap<String, Vec<String>> = HashMap::new();
    let mut before_key: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut inline: HashMap<(String, String), String> = HashMap::new();
    let mut header: Vec<String> = Vec::new();
    let footer: Vec<String>;
    let mut pending: Vec<String> = Vec::new();
    let mut section = String::new();

    for raw in old.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with('#') {
            pending.push(raw.to_string());
            continue;
        }
        if trimmed.is_empty() || raw.starts_with(char::is_whitespace) {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let header_text = trimmed.to_string();
            if section.is_empty() {
                header.append(&mut pending);
            } else {
                before_section.insert(header_text.clone(), std::mem::take(&mut pending));
            }
            section = header_text[1..header_text.len() - 1].to_string();
            continue;
        }
        let (code, comment) = match trimmed.find('#') {
            Some(i) => (trimmed[..i].trim_end(), Some(trimmed[i..].to_string())),
            None => (trimmed, None),
        };
        let key = code.split_once('=').map(|p| p.0).unwrap_or(code).trim().to_string();
        let anchor = (section.clone(), key);
        if let Some(c) = comment {
            inline.insert(anchor.clone(), c);
        }
        before_key.insert(anchor, std::mem::take(&mut pending));
    }
    footer = pending;

    // Comments attached to a key that was removed are kept under its section.
    let mut new_section = String::new();
    let mut new_keys: Vec<(String, String)> = Vec::new();
    for raw in new.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            new_section = trimmed[1..trimmed.len() - 1].to_string();
        } else if !trimmed.is_empty() && !raw.starts_with(char::is_whitespace) {
            let key = trimmed.split_once('=').map(|p| p.0).unwrap_or(trimmed).trim().to_string();
            new_keys.push((new_section.clone(), key));
        }
    }
    let mut orphans: HashMap<String, Vec<String>> = HashMap::new();
    for ((sec, key), comments) in &before_key {
        if !new_keys.contains(&(sec.clone(), key.clone())) && !comments.is_empty() {
            orphans.entry(sec.clone()).or_default().extend(comments.iter().cloned());
        }
    }

    let mut out: Vec<String> = Vec::new();
    out.extend(header);
    let mut sec = String::new();
    for raw in new.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if let Some(cs) = before_section.get(trimmed) {
                out.extend(cs.iter().cloned());
            }
            let name = trimmed[1..trimmed.len() - 1].to_string();
            if let Some(cs) = orphans.remove(&name) {
                out.extend(cs);
            }
            sec = name;
            out.push(raw.to_string());
            continue;
        }
        if trimmed.is_empty() || raw.starts_with(char::is_whitespace) {
            out.push(raw.to_string());
            continue;
        }
        let key = trimmed.split_once('=').map(|p| p.0).unwrap_or(trimmed).trim().to_string();
        let anchor = (sec.clone(), key);
        if let Some(cs) = before_key.get(&anchor) {
            out.extend(cs.iter().cloned());
        }
        if let Some(c) = inline.get(&anchor) {
            out.push(format!("{} {}", raw.trim_end(), c));
        } else {
            out.push(raw.to_string());
        }
    }
    out.extend(footer);
    let mut result = out.join("\n");
    result.push('\n');
    result
}

// Drop-in replacement for Ini::pretty_write() that keeps user comments.
pub fn profile_write(cfile: &str, profconf: &Ini, write_options: &WriteOptions) -> Result<(), Error> {
    let old = read_to_string(cfile).unwrap_or_default();
    let new = profconf.pretty_writes(write_options);
    std::fs::write(cfile, restore_profile_comments(&old, &new))
}

// Edit per-snapshot configuration
pub fn snapshot_config_edit(snapshot: &str) -> Result<(), Error> {
    // Make sure snapshot exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot edit configuration as snapshot {} doesn't exist.", snapshot)));
    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        // Make sure snapshot is not in use by another dsnap process
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.", snapshot, snapshot)));

    } else if snapshot == "0" {
        // Make sure is not base snapshot
        return Err(Error::new(ErrorKind::Unsupported,
                              "Changing base snapshot is not allowed.".to_string()));

    } else {
        // Edit dsnap config
        prepare(snapshot)?;
        if !std::env::var("EDITOR").unwrap_or_default().trim().is_empty() {
            let excode = Command::new("sh").arg("-c")
                          .arg(format!("$EDITOR /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/dsnap.conf", snapshot))
                          .status()?;
            if !excode.success() {
                eprintln!("The editor exited with an error; committing the configuration as it is.");
            }
        } else {
            // nano available
            if Command::new("sh").arg("-c")
                                 .arg("[ -x \"$(command -v nano)\" ]")
                                 .status()?.success() {
                                     Command::new("sh").arg("-c")
                                                       .arg(format!("nano /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/dsnap.conf", snapshot))
                                                       .status()?;
                                 }
            // vi available
            else if Command::new("sh").arg("-c")
                                      .arg("[ -x \"$(command -v vi)\" ]")
                                      .status()?.success() {
                                          Command::new("sh").arg("-c")
                                                            .arg(format!("vi /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/dsnap.conf", snapshot))
                                                            .status()?;
                                      }
            // vim available
            else if Command::new("sh").arg("-c")
                                      .arg("[ -x \"$(command -v vim)\" ]")
                                      .status()?.success() {
                                          Command::new("sh").arg("-c")
                                                            .arg(format!("vim /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/dsnap.conf", snapshot))
                                                            .status()?;
                                      }
            // neovim
            else if Command::new("sh").arg("-c")
                                      .arg("[ -x \"$(command -v nvim)\" ]")
                                      .status()?.success() {
                                          Command::new("sh").arg("-c")
                                                            .arg(format!("nvim /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/dsnap.conf", snapshot))
                                                            .status()?;
                                      }
            // micro
            else if Command::new("sh").arg("-c")
                                      .arg("[ -x \"$(command -v micro)\" ]")
                                      .status()?.success() {
                                          Command::new("sh").arg("-c")
                                                            .arg(format!("micro /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/dsnap.conf", snapshot))
                                                            .status()?;
                                      }
            else {
                chr_delete(snapshot)?;
                return Err(Error::new(ErrorKind::NotFound,
                                      "No text editor available!"));
            }
        }

        // Commit the edit to etc-N and clean up the chroot, otherwise the
        // edit is silently discarded by the next prepare()
        post_transactions(snapshot)?;
    }
    Ok(())
}

// Typed view of /etc/dsnap/dsnap.conf.
//
// On-disk format is unchanged: `key::value` lines, `,`/`::`-separated path
// lists, and `#` comments. This replaces the repeated split loops in callers.
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    pub aur: bool,
    pub mutable_dirs: Vec<String>,
    pub mutable_dirs_shared: Vec<String>,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            aur: false,
            mutable_dirs: Vec::new(),
            mutable_dirs_shared: vec!["var".to_string()],
        }
    }
}

impl SnapshotConfig {
    fn parse_dirs(value: &str) -> Vec<String> {
        value.split(',')
             .flat_map(|item| item.split("::"))
             .map(str::trim)
             .filter(|part| !part.is_empty())
             .map(String::from)
             .collect()
    }

    pub fn load(snapshot: &str) -> Result<Self, Error> {
        let mut config = Self::default();
        let path = format!("/.snapshots/etc/etc-{}/dsnap/dsnap.conf", snapshot);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(config),
            Err(e) => return Err(e),
        };

        for line in BufReader::new(file).lines() {
            let mut line = line?;
            let line = comment_after_hash(&mut line).trim();
            let Some((key, value)) = line.split_once("::") else { continue; };
            let key = key.trim();
            let mut value = value.trim_end().to_string();
            let value = comment_after_hash(&mut value).trim().to_string();

            match key {
                "aur" => {
                    config.aur = match value.to_ascii_lowercase().as_str() {
                        "false" => false,
                        "true" => true,
                        other => return Err(Error::new(ErrorKind::InvalidData,
                            format!("Invalid aur value '{}' in {}", other, path))),
                    };
                }
                "mutable_dirs" => config.mutable_dirs = Self::parse_dirs(&value),
                "mutable_dirs_shared" => config.mutable_dirs_shared = Self::parse_dirs(&value),
                unknown => eprintln!("Ignoring unknown dsnap config key '{}' in {}", unknown, path),
            }
        }
        Ok(config)
    }
}

pub fn snapshot_config(snapshot: &str) -> Result<SnapshotConfig, Error> {
    SnapshotConfig::load(snapshot)
}

// Get per-snapshot configuration options
pub fn snapshot_config_get(snapshot: &str) -> HashMap<String, String> {
    // Compatibility wrapper for callers not yet migrated.
    let config = snapshot_config(snapshot).unwrap_or_else(|e| {
        eprintln!("Failed to read snapshot {} configuration: {}", snapshot, e);
        SnapshotConfig::default()
    });
    let mut options = HashMap::new();
    #[cfg(feature = "pacman")]
    options.insert(String::from("aur"),
                   String::from(if config.aur { "True" } else { "False" }));
    options.insert(String::from("mutable_dirs"), config.mutable_dirs.join(","));
    options.insert(String::from("mutable_dirs_shared"), config.mutable_dirs_shared.join(","));
    options
}

// Show diff of packages between 2 snapshots
pub fn snapshot_diff(snapshot1: &str, snapshot2: &str) -> Result<(), Error> {
    // Make sure snapshot one does exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot1)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Snapshot {} not found.", snapshot1)));

        // Make sure snapshot two does exist
        } else if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot2)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Snapshot {} not found.", snapshot2)));

    } else {
        let snap1_pkgs = pkg_list(snapshot1, "")?;
        let snap2_pkgs = pkg_list(snapshot2, "")?;

        // Collect the missing packages names
        let mut missing_pkgs: Vec<String> = Vec::new();
        for pkg in &snap1_pkgs {
            if !snap2_pkgs.contains(&pkg) {
                missing_pkgs.push(format!("{} only in snapshot {}", pkg.to_string(),snapshot1));
            }
        }
        for pkg in &snap2_pkgs {
            if !snap1_pkgs.contains(&pkg) {
                missing_pkgs.push(format!("{} only in snapshot {}", pkg.to_string(),snapshot2));
            }
        }

        // Print the missing packages names
        if !missing_pkgs.is_empty() {
            missing_pkgs.sort();
            for name in missing_pkgs {
                println!("{}", name);
            }
        }

    }
    Ok(())
}

// Edit per-snapshot profile
pub fn snapshot_profile_edit(snapshot: &str) -> Result<(), Error> {
    // Make sure snapshot exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot edit profile as snapshot {} doesn't exist.", snapshot)));
    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        // Make sure snapshot is not in use by another dsnap process
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.", snapshot, snapshot)));

    } else if snapshot == "0" {
        // Make sure is not base snapshot
        return Err(Error::new(ErrorKind::Unsupported,
                              "Changing base snapshot is not allowed.".to_string()));

    } else {
        // Edit profile
        prepare(snapshot)?;

        // Launch editor
        if !std::env::var("EDITOR").unwrap_or_default().trim().is_empty() {
            let excode = Command::new("sh").arg("-c")
                          .arg(format!("$EDITOR /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot))
                          .status()?;
            if !excode.success() {
                eprintln!("The editor exited with an error; committing the profile as it is.");
            }
        } else {
            // nano available
            if Command::new("sh").arg("-c")
                                 .arg("[ -x \"$(command -v nano)\" ]")
                                 .status()?.success() {
                                     Command::new("sh").arg("-c")
                                                       .arg(format!("nano /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot))
                                                       .status()?;
                                 }
            // vi available
            else if Command::new("sh").arg("-c")
                                      .arg("[ -x \"$(command -v vi)\" ]")
                                      .status()?.success() {
                                          Command::new("sh").arg("-c")
                                                            .arg(format!("vi /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot))
                                                            .status()?;
                                      }
            // vim available
            else if Command::new("sh").arg("-c")
                                      .arg("[ -x \"$(command -v vim)\" ]")
                                      .status()?.success() {
                                          Command::new("sh").arg("-c")
                                                            .arg(format!("vim /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot))
                                                            .status()?;
                                      }
            // neovim
            else if Command::new("sh").arg("-c")
                                      .arg("[ -x \"$(command -v nvim)\" ]")
                                      .status()?.success() {
                                          Command::new("sh").arg("-c")
                                                            .arg(format!("nvim /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot))
                                                            .status()?;
                                      }
            // micro
            else if Command::new("sh").arg("-c")
                                      .arg("[ -x \"$(command -v micro)\" ]")
                                      .status()?.success() {
                                          Command::new("sh").arg("-c")
                                                            .arg(format!("micro /.snapshots/rootfs/snapshot-chr{}/etc/dsnap/profile", snapshot))
                                                            .status()?;
                                      }
            else {
                chr_delete(snapshot)?;
                return Err(Error::new(ErrorKind::NotFound,
                                      "No text editor available!"));
            }
        }

        if check_profile(snapshot).is_err() {
            chr_delete(snapshot)?;
            return Err(Error::new(ErrorKind::Other,
                                  "Failed to apply changes."));
        }

        // Commit the edit and clean up the chroot, otherwise the edit is
        // silently discarded by the next prepare()
        post_transactions(snapshot)?;
    }
    Ok(())
}

// Remove temporary chroot for specified snapshot only
// This unlocks the snapshot for use by other functions
pub fn snapshot_unlock(snapshot: &str) -> Result<(), Error> {
    let print_path = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    let path = Path::new(&print_path);
    if path.try_exists()? {
        // Make sure snapshot is not mounted
        if !is_mounted(path)? {
            let path_vec = vec!["boot", "etc", "var"];
            for path in path_vec {
                if Path::new(&format!("/.snapshots/{}/{}-chr{}", path,path,snapshot)).try_exists()? {
                    delete_subvolume(&format!("/.snapshots/{}/{}-chr{}", path,path,snapshot))?;
                }
            }
            // Try to remove directory contents if delete subvolume failed
            if delete_subvolume(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).is_err() {
                remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))?;
                delete_subvolume(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))?;
            }
        } else {
            umount2(Path::new(path),
                    MntFlags::MNT_DETACH)?;
            let path_vec = vec!["boot", "etc", "var"];
            for path in path_vec {
                if Path::new(&format!("/.snapshots/{}/{}-chr{}", path,path,snapshot)).try_exists()? {
                    delete_subvolume(&format!("/.snapshots/{}/{}-chr{}", path,path,snapshot))?;
                }
            }
            // Try to remove directory contents if delete subvolume failed
            if delete_subvolume(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).is_err() {
                remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))?;
                delete_subvolume(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))?;
            }
        }
    }
    Ok(())
}

// No comment
pub fn switch_to_windows() -> Result<ExitStatus, Error> {
    Command::new("efibootmgr").args(["-c", "-L", "Windows",
                                     "-l", "\\EFI\\BOOT\\BOOTX64.efi"])
                              .status()
}

// Fetch the response headers of a HEAD request, optionally skipping TLS
// certificate verification (needed when the clock is too far off for the
// certificate to validate).
fn fetch_date_headers(verify_tls: bool) -> Result<String, Error> {
    // curl --proto =https -I https://google.com
    let mut easy = Easy::new();
    easy.url("https://google.com")?;

    easy.http_version(HttpVersion::V2)?;
    // Let curl negotiate TLS 1.2/1.3 instead of forcing 1.3 only
    easy.nobody(true)?;
    easy.connect_timeout(Duration::from_secs(10))?;
    easy.timeout(Duration::from_secs(20))?;

    if !verify_tls {
        easy.ssl_verify_peer(false)?;
        easy.ssl_verify_host(false)?;
    }

    let mut headers = List::new();
    headers.append("Accept: */*")?;
    easy.http_headers(headers)?;

    let mut response_headers = Vec::new();
    {
        let mut transfer = easy.transfer();
        // Collect headers only, the body is not downloaded (nobody = true)
        transfer.header_function(|header| {
            response_headers.extend_from_slice(header);
            true
        })?;
        transfer.perform()?;
    }

    Ok(String::from_utf8_lossy(&response_headers).into_owned())
}

// Sync time
pub fn sync_time() -> Result<(), Error> {
    // Normally verify the certificate, but the clock is what we are trying to
    // fix: if it is wrong enough the certificate check fails, so retry
    // without verification instead of failing to sync at all.
    let response_headers_str = match fetch_date_headers(true) {
        Ok(headers) => headers,
        Err(err) => {
            eprintln!("Verified TLS request failed ({}), retrying without certificate verification.", err);
            fetch_date_headers(false)?
        }
    };

    let date_header = response_headers_str
        .lines()
        .find(|line| line.to_lowercase().starts_with("date:"))
        .ok_or_else(|| Error::new(ErrorKind::Other, "Date header not found."))?;

    // HTTP date headers look like "date: Tue, 17 Nov 2026 13:07:43 GMT"
    // and are always in GMT (RFC 7231), tokens 2..7 include the timezone
    let date_str: Vec<&str> = date_header.split_whitespace().collect();
    if date_str.len() < 7 {
        return Err(Error::new(ErrorKind::Other, "Malformed date header."));
    }
    let date = &date_str[2..7].join(" ");

    // Set time and make sure it actually worked
    let output = Command::new("date").args(["-u", "-s"]).arg(date).output()?;
    if !output.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to set time: {}",
                                      String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(())
}

// Clear all temporary snapshots
pub fn temp_snapshots_clear() -> Result<(), Error> {
    // Collect snapshot paths
    let mut snapshots: Vec<PathBuf> = Vec::new();
    for dir in ["/.snapshots/boot", "/.snapshots/etc", "/.snapshots/var", "/.snapshots/rootfs"] {
        match read_dir(dir) {
            Ok(entries) => {
                for entry in entries {
                    snapshots.push(entry?.path());
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }

    // Clear temp if exist
    for snapshot in snapshots {
        let s = snapshot.to_string_lossy();
        let is_temp = s.contains("snapshot-chr")
            || s.contains("var-chr")
            || s.contains("etc-chr")
            || s.contains("boot-chr");
        if !is_temp {
            continue;
        }
        if !is_mounted(&snapshot)? {
            delete_subvolume(&s)?;
        } else {
            eprintln!("{} is busy.", s);
        }
    }
    Ok(())
}

// Clean tmp dirs
pub fn tmp_delete(secondary: bool) -> Result<(), Error> {
    // Get tmp
    let tmp = get_tmp();
    let tmp = get_aux_tmp(tmp, secondary);

    // Clean tmp
    if Path::new(&format!("/.snapshots/rootfs/snapshot-{}", tmp)).try_exists()? {
        delete_subvolume(&format!("/.snapshots/boot/boot-{}", tmp))?;
        delete_subvolume(&format!("/.snapshots/etc/etc-{}", tmp))?;
        delete_subvolume(&format!("/.snapshots/var/var-{}", tmp))?;
        delete_subvolume(&format!("/.snapshots/rootfs/snapshot-{}", tmp))?;
    }
    Ok(())
}

// Recursively install package in tree
pub fn tree_install(treename: &str, pkgs: &Vec<String>, profiles: &Vec<String>, force: bool
                    ,user_profiles: &Vec<String>, noconfirm: bool, secondary: bool) -> Result<(), Error> {
    // Make sure treename exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", treename)).try_exists()? {
        eprintln!("Cannot remove as tree {} doesn't exist.", treename);

    } else {
        // Import tree value
        let tree = fstree()?;
        // Install packages
        if !pkgs.is_empty() {
            for pkg in pkgs {
                install(treename, &vec![pkg.to_string()], noconfirm)?;
                let order = recurse_tree(&tree, treename)?;
                for branch in order {
                    if branch != treename {
                        println!("{}, {}", treename,branch);
                        install(&branch, &vec![pkg.to_string()], noconfirm)?;
                    }
                }
            }
        } else if !profiles.is_empty() {
            // Install profiles
            for profile in profiles {
                let user_profile = "";
                if install_profile(treename, &profile, force, secondary, &user_profile, noconfirm).is_ok() {
                    post_transactions(treename)?;
                } else {
                    chr_delete(treename)?;
                    return Err(Error::new(ErrorKind::Other,
                                          format!("Failed to install and changes discarded.")));
                }
                let order = recurse_tree(&tree, treename)?;
                for branch in order {
                    if branch != treename {
                        println!("{}, {}", treename,branch);
                        if install_profile(&branch, &profile, force, secondary, &user_profile, noconfirm).is_ok() {
                            post_transactions(&branch)?;
                        } else {
                            chr_delete(&branch)?;
                            return Err(Error::new(ErrorKind::Other,
                                                  format!("Failed to install and changes discarded.")));
                        }
                    }
                }
            }
        } else if !user_profiles.is_empty() {
            // Install profiles
            for user_profile in user_profiles {
                let profile = "";
                if install_profile(treename, &profile, force, secondary, &user_profile, noconfirm).is_ok() {
                    post_transactions(treename)?;
                } else {
                    chr_delete(treename)?;
                    return Err(Error::new(ErrorKind::Other,
                                          format!("Failed to install and changes discarded.")));
                }
                let order = recurse_tree(&tree, treename)?;
                for branch in order {
                    if branch != treename {
                        println!("{}, {}", treename,branch);
                        if install_profile(&branch, &profile, force, secondary, &user_profile, noconfirm).is_ok() {
                            post_transactions(&branch)?;
                        } else {
                            chr_delete(&branch)?;
                            return Err(Error::new(ErrorKind::Other,
                                                  format!("Failed to install and changes discarded.")));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// Recursively remove package in tree
pub fn tree_remove(treename: &str, pkgs: &Vec<String>, profiles: &Vec<String>, user_profiles: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    // Make sure treename exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", treename)).try_exists()? {
        eprintln!("Cannot remove as tree {} doesn't exist.", treename);

    } else {
        // Import tree value
        let tree = fstree()?;
        // Remove packages
        if !pkgs.is_empty() {
            for pkg in pkgs {
                uninstall(treename, &vec![pkg.to_string()], noconfirm)?;
                let order = recurse_tree(&tree, treename)?;
                for branch in order {
                    if branch != treename {
                        println!("{}, {}", treename,branch);
                        uninstall(&branch, &vec![pkg.to_string()], noconfirm)?;
                    }
                }
            }
        } else if !profiles.is_empty() {
            // Remove profiles
            for profile in profiles {
                let user_profile = "";
                if uninstall_profile(treename, &profile, &user_profile, noconfirm).is_ok() {
                    post_transactions(treename)?;
                } else {
                    chr_delete(treename)?;
                    return Err(Error::new(ErrorKind::Other,
                                          format!("Failed to remove and changes discarded.")));
                }
                let order = recurse_tree(&tree, treename)?;
                for branch in order {
                    if branch != treename {
                        println!("{}, {}", treename,branch);
                        if uninstall_profile(&branch, &profile, &user_profile, noconfirm).is_ok() {
                            post_transactions(&branch)?;
                        } else {
                            chr_delete(&branch)?;
                            return Err(Error::new(ErrorKind::Other,
                                                  format!("Failed to remove and changes discarded.")));
                        }
                    }
                }
            }
        } else if !user_profiles.is_empty() {
            // Remove profiles
            for user_profile in user_profiles {
                let profile = "";
                if uninstall_profile(treename, &profile, &user_profile, noconfirm).is_ok() {
                    post_transactions(treename)?;
                } else {
                    chr_delete(treename)?;
                    return Err(Error::new(ErrorKind::Other,
                                          format!("Failed to remove and changes discarded.")));
                }
                let order = recurse_tree(&tree, treename)?;
                for branch in order {
                    if branch != treename {
                        println!("{}, {}", treename,branch);
                        if uninstall_profile(&branch, &profile, &user_profile, noconfirm).is_ok() {
                            post_transactions(&branch)?;
                        } else {
                            chr_delete(&branch)?;
                            return Err(Error::new(ErrorKind::Other,
                                                  format!("Failed to remove and changes discarded.")));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// Recursively run a command in tree
pub fn tree_run(treename: &str, cmd: &str) -> Result<(), Error> {
    // Make sure treename exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", treename)).try_exists()? {
                return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot update as tree {} doesn't exist.", treename)));

    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", treename)).try_exists()? {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}.",
                                      treename,treename)));
    } else {
        // Run command
        prepare(treename)?;
        chroot_exec(&format!("/.snapshots/rootfs/snapshot-chr{}", treename), cmd)?;
        post_transactions(treename)?;

        // Import tree file
        let tree = fstree()?;

        let order = recurse_tree(&tree, treename)?;
        for branch in order {
            if branch != treename {
                println!("{}, {}", treename,branch);
                if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", branch)).try_exists()? {
                    return Err(Error::new(ErrorKind::Unsupported,
                                          format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}.",
                                                  branch,branch)));
                } else {
                    prepare(&branch)?;
                    chroot_exec(&format!("/.snapshots/rootfs/snapshot-chr{}", branch), cmd)?;
                    post_transactions(&branch)?;
                }
            }
        }
    }
    Ok(())
}

// Calls print function
pub fn tree_show() -> Result<(), Error> {
    // Import tree file
    let tree = fstree().map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    tree_print(&tree)
}

// Sync tree and all its snapshots
pub fn tree_sync(treename: &str, live: bool) -> Result<(), Error> {
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", treename)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot sync as tree {} doesn't exist.", treename)));

    // Make sure snapshot is not in use by another dsnap process
    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", treename)).try_exists()? {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}.",
                                      treename,treename)));
    } else {
        // Syncing tree automatically updates it
        if tree_upgrade(treename).is_err() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to upgrade tree {}.", treename)));
        }

        // Import tree file
        let tree = fstree()?;

        let order = recurse_tree(&tree, treename)?;
        for branch in order {
            if branch != treename {
                println!("{}, {}", treename,branch);
                if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", branch)).try_exists()? {
                    return Err(Error::new(ErrorKind::Unsupported,
                                          format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}.",
                                                  branch,branch)));
                } else {
                    prepare(&branch)?;
                    // Pre-sync
                    tree_sync_helper(treename, &branch, "chr")?;
                    // Live sync
                    if live && branch == get_current_snapshot()? {
                        // Post-sync
                        tree_sync_helper(&branch, &get_tmp(), "")?;
                    }
                    // Moved here from the line immediately after first sync_tree_helper
                    post_transactions(&branch)?;
                }
            }
        }
    }
    Ok(())
}

// Recursively run an update in tree
pub fn tree_upgrade(treename: &str) -> Result<(), Error> {
    // Make sure treename exist
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", treename)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot update as tree {} doesn't exist.", treename)));

        // Make sure snapshot is not in use by another dsnap process
        } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", treename)).try_exists()? {
        return Err(
            Error::new(ErrorKind::Unsupported,
                       format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                               treename,treename)));

    } else {
        // Run update
        if auto_upgrade(treename).is_err() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to auto upgrade tree {}.", treename)));
        };

        // Import tree file
        let tree = fstree()?;

        let order = recurse_tree(&tree, treename)?;

        // Auto upgrade braches in sync_tree
        for branch in order {
            if branch != treename {
                println!("{}, {}", treename,branch);
                // Make sure snapshot is not in use by another dsnap process
                if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", branch)).try_exists()? {
                    return Err(
                        Error::new(ErrorKind::Unsupported,
                                   format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                                           branch,branch)));
                }
                // Run update
                if auto_upgrade(&branch).is_err() {
                    return Err(Error::new(ErrorKind::Other,
                                          format!("Failed to auto upgrade tree {}.",  branch)));
                }
            }
        }
    }
    Ok(())
}

// Uninstall package(s)
pub fn uninstall(snapshot: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot remove as snapshot {} doesn't exist.", snapshot)));

        // Make sure snapshot is not in use by another dsnap process
        } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(
            Error::new(ErrorKind::Unsupported,
                       format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                               snapshot,snapshot)));

        // Make sure snapshot is not base snapshot
        } else if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Changing base snapshot is not allowed.")));

    } else {
        // Uninstall package
        prepare(snapshot)?;
        if let Err(e) = uninstall_package_helper(snapshot, &pkgs, noconfirm) {
            let _ = chr_delete(snapshot);
            return Err(Error::new(ErrorKind::Interrupted,
                                  format!("Remove failed and changes discarded: {}", e)));
        }
        if let Err(e) = post_transactions(snapshot) {
            let _ = chr_delete(snapshot);
            return Err(Error::new(ErrorKind::Other,
                                  format!("Remove failed and changes discarded: {}", e)));
        }
        println!("Package(s) {pkgs:?} removed from snapshot {} successfully.", snapshot);
    }
    Ok(())
}

// Uninstall live
pub fn uninstall_live(pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    let tmp = get_tmp();
    dsnap_mounts(&tmp, "").map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let run = uninstall_package_helper_live(&tmp, &pkgs, noconfirm);
    // Umount even when the uninstall failed, otherwise the mounts leak
    if let Err(e) = dsnap_umounts(&tmp, "") {
        if run.is_ok() {
            return Err(Error::new(ErrorKind::Other, e.to_string()));
        }
        eprintln!("Failed to unmount live snapshot {}: {}", tmp, e);
    }
    run
}

// Uninstall a profile from a text file
fn uninstall_profile(snapshot: &str, profile: &str, user_profile: &str, noconfirm: bool) -> Result<(), Error> {
    // Get some values
    let distro = detect::distro_id();
    let dist_name = if distro.contains("_ashos") {
        distro.replace("_ashos", "")
    } else {
        distro
    };
    let cfile = format!("/usr/share/dsnap/profiles/{}/{}", profile,dist_name);

    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot uninstall as snapshot {} doesn't exist.", snapshot)));

        // Make sure snapshot is not in use by another dsnap process
        } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        return Err(
            Error::new(ErrorKind::Unsupported,
                       format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                               snapshot,snapshot)));

        // Make sure snapshot is not base snapshot
        } else if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Changing base snapshot is not allowed.")));
    } else {
        // Uninstall profile
        // Prepare
        prepare(snapshot)?;
        let run = (|| -> Result<(), Error> {
        // Profile configurations
        let mut profconf = Ini::new_cs();
        profconf.set_comment_symbols(&['#']);
        profconf.set_multiline(true);
        // Load profile if exist
        if Path::new(&cfile).is_file() && user_profile.is_empty() {
            profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        } else if !user_profile.is_empty() {
            profconf.load(user_profile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        }

        // Read commands section in configuration file
        if profconf.sections().contains(&"uninstall-commands".to_string()) {
            let section = profconf.get_map_ref().get("uninstall-commands")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing uninstall-commands section."))?;
            for cmd in section.keys() {
                if !cmd.trim().is_empty() {
                    chroot_exec(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot), cmd)?;
                }
            }
        }

        // Read packages section in configuration file
        if profconf.sections().contains(&"profile-packages".to_string()) {
            let section = profconf.get_map_ref().get("profile-packages")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing profile-packages section."))?;
            let mut pkgs: Vec<String> = section.keys().cloned().collect();
            pkgs.retain(|p| !p.trim().is_empty());
            // Uninstall package(s)
            uninstall_package_helper(snapshot, &pkgs, noconfirm)?;
        }

        // Ignore empty file
        if !profconf.sections().contains(&"profile-packages".to_string()) &&
            !profconf.sections().contains(&"uninstall-commands".to_string())
        {
            return Err(Error::new(ErrorKind::InvalidInput,
                                  "The profile is empty or invalid.".to_string()));
        }
        Ok(())
        })();
        if run.is_err() {
            let _ = chr_delete(snapshot);
        }
        run
    }
}

// Uninstall profile in live snapshot
fn uninstall_profile_live(_snapshot: &str,profile: &str, user_profile: &str, noconfirm: bool) -> Result<(), Error> {
    // Get some values
    let distro = detect::distro_id();
    let dist_name = if distro.contains("_ashos") {
        distro.replace("_ashos", "")
    } else {
        distro
    };
    let cfile = format!("/usr/share/dsnap/profiles/{}/{}", profile,dist_name);
    let tmp = get_tmp();

    // Prepare
    dsnap_mounts(&tmp, "").map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let run = (|| -> Result<(), Error> {

    // Profile configurations
    let mut profconf = Ini::new_cs();
    profconf.set_comment_symbols(&['#']);
    profconf.set_multiline(true);

    // Load profile if exist
    if Path::new(&cfile).is_file() && user_profile.is_empty() {
        profconf.load(&cfile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    } else if !user_profile.is_empty() {
        profconf.load(user_profile).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    }

    // Read packages section in configuration file
    if profconf.sections().contains(&"profile-packages".to_string()) {
        let section = profconf.get_map_ref().get("profile-packages")
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing profile-packages section."))?;
        let mut pkgs: Vec<String> = section.keys().cloned().collect();
        pkgs.retain(|p| !p.trim().is_empty());
        // Uninstall package(s)
        uninstall_package_helper_live(&tmp, &pkgs, noconfirm)?;
    }

    // Read commands section in configuration file
    if profconf.sections().contains(&"uninstall-commands".to_string()) {
        let section = profconf.get_map_ref().get("uninstall-commands")
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid profile: missing uninstall-commands section."))?;
        for cmd in section.keys() {
            if !cmd.trim().is_empty() {
                chroot_exec(&format!("/.snapshots/rootfs/snapshot-{}", tmp), cmd)?;
            }
        }
    }

    // Ignore empty file
    if !profconf.sections().contains(&"profile-packages".to_string()) &&
        !profconf.sections().contains(&"uninstall-commands".to_string())
    {
        return Err(Error::new(ErrorKind::InvalidInput,
                              "The profile is empty or invalid.".to_string()));
    }
    Ok(())
    })();

    // Umount tmp: the mounts must be released even when the uninstall
    // failed partway
    if let Err(e) = dsnap_umounts(&tmp, "") {
        if run.is_ok() {
            return Err(Error::new(ErrorKind::Other, e.to_string()));
        }
        eprintln!("Failed to unmount live snapshot {}: {}", tmp, e);
    }

    run
}

// Triage functions for argparse method
pub fn uninstall_triage(snapshot: &str, live: bool, pkgs: Vec<String>, profile: &str,
                        user_profile: &str, noconfirm: bool) -> Result<(), Error> {
    // Switch between profile and user_profile
    let p = if user_profile.is_empty() {
        profile
    } else {
        user_profile
    };

    if !live {
        // Uninstall profile
        if !profile.is_empty() {
            uninstall_profile(snapshot, profile, user_profile, noconfirm)?;
            if post_transactions(snapshot).is_ok() {
                println!("Profile {} removed from snapshot {} successfully.", p, snapshot);
            } else {
                let _ = chr_delete(snapshot);
                return Err(Error::new(ErrorKind::Other,
                                      "Uninstall failed and changes discarded.".to_string()));
            }

        } else if !pkgs.is_empty() {
            // Uninstall package
            uninstall(snapshot, &pkgs, noconfirm)?;

        } else if !user_profile.is_empty() {
            // Uninstall user_profile
            uninstall_profile(snapshot, profile, user_profile, noconfirm)?;
            if post_transactions(snapshot).is_ok() {
                println!("Profile {} removed from snapshot {} successfully.", p, snapshot);
            } else {
                let _ = chr_delete(snapshot);
                return Err(Error::new(ErrorKind::Other,
                                      "Uninstall failed and changes discarded.".to_string()));
            }
        }

    } else if live && snapshot != &get_current_snapshot()? {
        // Prevent live Uninstall except for current snapshot
        return Err(Error::new(ErrorKind::Unsupported,
                              "Can't use the live option with any other snapshot than the current one.".to_string()));

    // Do live uninstall only if: live flag is used OR target snapshot is current
    } else if live && snapshot == &get_current_snapshot()? {
        if !profile.is_empty() {
            // Live profile uninstall
            uninstall_profile_live(snapshot, profile, user_profile, noconfirm)?;
            println!("Profile {} removed from current/live snapshot.", p);

        } else if !pkgs.is_empty() {
            // Live package uninstall
            uninstall_live(&pkgs, noconfirm)?;
            println!("Package(s) {pkgs:?} removed from snapshot {} successfully.", snapshot);

        } else if !user_profile.is_empty() {
            // Live user_profile uninstall
            uninstall_profile_live(snapshot, profile, user_profile, noconfirm)?;
            println!("Profile {} removed from current/live snapshot.", p);
        }
    }
    Ok(())
}

// Saves changes made to /etc to snapshot
pub fn update_etc() -> Result<(), Error> {
    let snapshot = get_current_snapshot()?;
    let tmp = get_tmp();

    // Make sure snapshot is not base snapshot
    if snapshot == "0" {
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Changing base snapshot is not allowed.")));
    } else {
        // Remove old /etc
        delete_subvolume(&format!("/.snapshots/etc/etc-{}", snapshot))?;

        // Check mutability
        let immutability = if check_mutability(&snapshot) {
            false
        } else {
            true
        };

        // Create new /etc
        create_snapshot(&format!("/.snapshots/etc/etc-{}", tmp),
                        &format!("/.snapshots/etc/etc-{}", snapshot),
                        immutability)?;
    }
    Ok(())
}

// Upgrade snapshot
pub fn upgrade(snapshot:  &str, baseup: bool, noconfirm: bool) -> Result<(), Error> {
    // Make sure snapshot exists
    if !Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists()? {
        return Err(Error::new(ErrorKind::NotFound,
                              format!("Cannot upgrade as snapshot {} doesn't exist.", snapshot)));

    } else if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists()? {
        // Make sure snapshot is not in use by another dsnap process
        return Err(
            Error::new(ErrorKind::Unsupported,
                       format!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                               snapshot,snapshot)));

    } else if snapshot == "0" && !baseup {
        // Make sure snapshot is not base snapshot
        return Err(Error::new(ErrorKind::Unsupported,
                              format!("Changing base snapshot is not allowed.")));

    } else {
        // Default upgrade behaviour is now "safe" update, meaning failed updates get fully discarded
        let excode = upgrade_helper(snapshot, noconfirm);
        if excode.is_ok() {
            if post_transactions(snapshot).is_err() {
                chr_delete(snapshot)?;
                return Err(Error::new(ErrorKind::Other,
                                      format!("Post-upgrade cleanup failed and changes discarded.")));
            }
            if baseup {
                if deploy_recovery(snapshot).is_err() {
                    return Err(Error::new(ErrorKind::Other,
                                          format!("Failed to deploy recovery snapshot.")));
                }
            }
            println!("Snapshot {} upgraded successfully.", snapshot);
        } else {
            chr_delete(snapshot)?;
            return Err(Error::new(ErrorKind::Other,
                                  format!("Upgrade failed and changes discarded.")));
        }
    }
    Ok(())
}

// Return snapshot that has a package
pub fn which_snapshot_has(pkgs: Vec<String>) -> Result<(), Error> {
    // Collect snapshots
    let tree = fstree()?;
    let snapshots = return_children(&tree, "root")?;

    // Search snapshots for package
    for pkg in pkgs {
        let mut snapshot: Vec<String> = Vec::new();
        for snap in &snapshots {
            if is_pkg_installed(&snap.to_string(), &pkg)? {
                snapshot.push(format!("{}", snap.to_string()));
            }
        }
        if !snapshot.is_empty() {
            println!("Package(s) {} installed in {snapshot:?}.", pkg);
        }
    }
    Ok(())
}

// Write new description (default) or append to an existing one (i.e. toggle immutability)
pub fn write_desc(snapshot: &str, desc: &str, overwrite: bool) -> Result<(), Error> {
    let mut descfile = if overwrite {
        OpenOptions::new().truncate(true)
                          .create(true)
                          .read(true)
                          .write(true)
                          .open(format!("/.snapshots/dsnap/snapshots/{}-desc", snapshot))?
    } else {
        OpenOptions::new().append(true)
                          .create(true)
                          .read(true)
                          .open(format!("/.snapshots/dsnap/snapshots/{}-desc", snapshot))?
    };
    descfile.write_all(desc.as_bytes())?;
    Ok(())
}

// Generic yes no prompt
pub fn yes_no(msg: &str) -> bool {
    loop {
        println!("{} (y/n)", msg);
        let mut reply = String::new();
        if let Err(e) = stdin().read_line(&mut reply) {
            eprintln!("Failed to read input: {}", e);
            return false;
        }
        match reply.trim().to_lowercase().as_str() {
            "n" | "no" => return false,
            "y" | "yes" => return true,
            _ => eprintln!("Invalid choice!"),
        }
    }
}
