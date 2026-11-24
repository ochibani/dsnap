use crate::{audit_log, check_profile, chr_delete, chroot_exec, cp_reflink, cp_reflink_no_clobber,
            get_tmp, grub, has_systemd, is_system_locked, is_system_pkg, post_transactions, prepare,
            profile_write, remove_dir_content, run_chroot, sync_time, validate_package_specs,
            validate_service_names, validate_system_config_path};

use configparser::ini::{Ini, WriteOptions};
use rustix::path::Arg;
use std::fs::{metadata, OpenOptions, read_to_string};
use std::io::{Error, ErrorKind, Write};
use std::path::Path;
use std::process::Command;

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

        // Use dnf with argv, never through a shell
        let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
        let excode = Command::new("chroot").arg(&root)
                                           .args(["dnf", "-y", "upgrade", "--refresh"])
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
    Ok(())
}

// Reinstall base packages in snapshot
pub fn bootstrap(snapshot: &str) -> Result<(), Error> {
    let release = code_name(snapshot);
    let target_path = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);

    // Bootstrap command
    let excode = Command::new("dnf")
        .arg(format!("--installroot={}", target_path))
        .args(["install", "-y", "basesystem", "dnf", "glibc-all-langpacks"])
        .arg(format!("--releasever={}", release))
        .status()?;

    if !excode.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to bootstrap."));
    }
    Ok(())
}

// Copy cache of downloaded packages to shared
pub fn cache_copy(snapshot: &str, prepare: bool) -> Result<(), Error> {
    let tmp = get_tmp();
    if prepare {
        // Populate the chroot cache from the deployed snapshot's cache
        cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/var/cache/dnf/.", tmp),
                   format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/dnf", snapshot))?;
    } else {
        // Persist the chroot cache back to the deployed snapshot
        cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/dnf/.", snapshot),
                   format!("/.snapshots/rootfs/snapshot-{}/var/cache/dnf", tmp))?;
    }
    Ok(())
}

// Clean dnf cache
pub fn clean_cache(snapshot: &str) -> Result<(), Error> {
    if Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/dnf", snapshot)).try_exists()? {
        remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/dnf", snapshot))?;
    }
    Ok(())
}

// Distribution version identifier used as --releasever
pub fn code_name(snapshot: &str) -> String {
    let mut code_name = String::new();
    // Check if /etc/os-release exists and contains VERSION_ID
    if code_name.is_empty() {
        if let Ok(file) = read_to_string(&format!("/.snapshots/rootfs/snapshot-{}/etc/os-release", snapshot)) {
            for line in file.lines() {
                if line.starts_with("VERSION_ID") {
                    if let Some((_, value)) = line.split_once('=') {
                        code_name = value.to_lowercase().trim_matches('"').to_string();
                        break;
                    }
                }
            }
        }
    }

    code_name
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
        // Install packages using dnf without involving a shell
        let mut install_args = vec!["install"];
        if noconfirm {
            install_args.push("-y");
        }
        install_args.extend_from_slice(&["--setopt", "install_weak_deps=False", "--refresh"]);
        install_args.push(pkg.as_str());
        let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
        if run_chroot(&root, "dnf", &install_args).is_err() {
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
    }
    Ok(())
}

// Install atomic-operation
pub fn install_package_helper_chroot(snapshot:&str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    validate_package_specs(pkgs)?;
    let packages = pkgs.join(" ");
    audit_log("install-packages-chroot", &[("snapshot", snapshot), ("packages", packages.as_str())]);
    let mut install_args = vec!["install"];
    if noconfirm {
        install_args.push("-y");
    }
    install_args.extend_from_slice(&["--setopt", "install_weak_deps=False", "--refresh"]);
    for pkg in pkgs {
        install_args.push(pkg.as_str());
    }

    let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    if run_chroot(&root, "dnf", &install_args).is_err() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to install package(s)."));
    }
    Ok(())
}

// Install atomic-operation in live snapshot
pub fn install_package_helper_live(_snapshot: &str, tmp: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    validate_package_specs(pkgs)?;
    let packages = pkgs.join(" ");
    audit_log("install-packages-live", &[("snapshot", tmp), ("packages", packages.as_str())]);
    let root = format!("/.snapshots/rootfs/snapshot-{}", tmp);
    for pkg in pkgs {
        let mut install_args = vec!["install"];
        if noconfirm {
            install_args.push("-y");
        }
        install_args.extend_from_slice(&["--setopt", "install_weak_deps=False", "--refresh"]);
        install_args.push(pkg.as_str());
        if run_chroot(&root, "dnf", &install_args).is_err() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to install {}", pkg)));
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
    let mut system_pkgs: Vec<String> = Vec::new();
    if let Some(section) = profconf.get_map().and_then(|map| map.get("system-packages").cloned()) {
        for pkg in section.keys() {
            system_pkgs.push(pkg.to_string());
        }
    }

    let mut lockpkg = String::new();
    if !system_pkgs.is_empty() {
        for pkg in system_pkgs {
            let rule = format!("{}\n", pkg);
            lockpkg.push_str(&rule);
        }
    }
    let mut rule_file = OpenOptions::new().truncate(true)
                                          .create(true)
                                          .read(true)
                                          .write(true)
                                          .open(format!("/.snapshots/rootfs/snapshot-chr{}/etc/dnf/protected.d/dsnap_system_packages.conf", snapshot))?;
    rule_file.write_all(lockpkg.as_bytes())?;
    Ok(())
}

fn parse_pkg_names(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

// Get list of user-installed packages, excluding dependency-only packages
pub fn no_dep_pkg_list(snapshot: &str, chr: &str) -> Result<Vec<String>, Error> {
    let root = format!("/.snapshots/rootfs/snapshot-{}{}", chr, snapshot);
    let excode = Command::new("chroot")
                          .arg(root)
                          .args(["dnf", "repoquery", "-C", "--userinstalled", "--qf=%{name}"])
                          .output()?;
    if !excode.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to list packages: {}",
                                      String::from_utf8_lossy(&excode.stderr).trim())));
    }
    Ok(parse_pkg_names(&excode.stdout))
}

// Get list of packages installed in a snapshot
pub fn pkg_list(snapshot: &str, chr: &str) -> Result<Vec<String>, Error> {
    prepare(snapshot)?;
    let root = format!("/.snapshots/rootfs/snapshot-{}{}", chr, snapshot);
    let output = Command::new("chroot")
                          .arg(root)
                          .args(["dnf", "repoquery", "-C", "--installed", "--qf=%{name}"])
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
    Ok(parse_pkg_names(&excode.stdout))
}

// DNF query
pub fn pkg_query(pkg: &str) -> Result<String, Error> {
    let output = Command::new("rpm")
                      .args(["-q", "--queryformat", "%{NAME} %{VERSION}", pkg])
                      .output()?;
    if !output.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Package {} not found.", pkg)));
    }
    // rpm -q --queryformat prints "pkg version"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.split_whitespace().nth(1)
                        .ok_or_else(|| Error::new(ErrorKind::Other,
                                                   format!("Failed to parse version for package {}.", pkg)))?;
    Ok(version.to_string())
}

// Refresh snapshot atomic-operation
pub fn refresh_helper(snapshot: &str) -> Result<(), Error> {
    let refresh = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                        .args(["dnf", "makecache"])
                                        .status()?;
    if !refresh.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Refresh failed."));
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
        } //TODO add OspenRC
    }
    Ok(())
}

// Copy system configurations to new snapshot
pub fn system_config(snapshot: &str, profconf: &Ini) -> Result<(), Error> {
    // Copy [fstab, time ,localization, network configuration, users and groups]
    let files = vec!["/etc/fstab", "/etc/localtime", "/etc/adjtime", "/etc/locale.gen", "/etc/locale.conf",
                     "/etc/vconsole.conf", "/etc/hostname", "/etc/shadow", "/etc/passwd", "/etc/gshadow",
                     "/etc/group", "/etc/sudoers"];
    for file in files {
        if Path::new(&format!("/.snapshots/rootfs/snapshot-{}{}", snapshot,file)).is_file() {
            cp_reflink(format!("/.snapshots/rootfs/snapshot-{}{}", snapshot,file),
                       format!("/.snapshots/rootfs/snapshot-chr{}{}", snapshot,file))?;
        }
    }

    // Copy dnf configuration
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/etc/dnf", snapshot))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/etc/dnf/.", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/etc/dnf/", snapshot))?;
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/etc/yum.repos.d", snapshot))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/etc/yum.repos.d/.", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/etc/yum.repos.d/", snapshot))?;

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
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/var/cache/dnf/.", s_f),
               format!("/.snapshots/rootfs/snapshot-{}{}/var/cache/dnf", chr,s_t))?;
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
        let uninstall_args: Vec<&str> = if noconfirm {
            vec!["dnf", "remove", "-y"]
        } else {
            vec!["dnf", "remove"]
        };

        if !is_system_locked() || !is_system_pkg(&profconf, pkg.to_string()) {
            let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                               .args(&uninstall_args)
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
    let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    for pkg in pkgs {
        let mut uninstall_args = vec!["remove"];
        if noconfirm {
            uninstall_args.push("-y");
        }
        uninstall_args.push(pkg.as_str());
        if run_chroot(&root, "dnf", &uninstall_args).is_err() {
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
        let uninstall_args: Vec<&str> = if noconfirm {
            vec!["dnf", "remove", "-y"]
        } else {
            vec!["dnf", "remove"]
        };

        let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-{}", tmp))
                                           .args(&uninstall_args)
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

    let mut upgrade_args = vec!["upgrade"];
    if noconfirm {
        upgrade_args.push("-y");
    }
    upgrade_args.push("--refresh");

    // Run dnf upgrade
    let root = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    if run_chroot(&root, "dnf", &upgrade_args).is_err() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to upgrade snapshot {}.", snapshot)));
    }
    Ok(())
}

// Live upgrade snapshot atomic-operation
pub fn upgrade_helper_live(tmp: &str, noconfirm: bool) -> Result<(), Error> {
    let mut upgrade_args = vec!["upgrade"];
    if noconfirm {
        upgrade_args.push("-y");
    }
    upgrade_args.push("--refresh");

    // Run dnf upgrade
    let root = format!("/.snapshots/rootfs/snapshot-{}", tmp);
    if run_chroot(&root, "dnf", &upgrade_args).is_err() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to upgrade current/live snapshot."));
    }
    Ok(())
}
