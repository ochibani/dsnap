use crate::{check_profile, chr_delete, chroot_exec, cp_reflink, cp_reflink_no_clobber, get_tmp, grub,
            has_systemd, is_system_locked, is_system_pkg, post_transactions, prepare, profile_write,
            remove_dir_content, sync_time};

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


        // Use apt
        let options = "-o Dpkg::Options::=\"--force-confdef\" -o Dpkg::Options::=\"--force-confold\"";
        let apt_update = "apt-get update";
        let apt_upgrade = format!("apt-get {} -y upgrade", options);
        let update = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} {}", snapshot,apt_update))
                                       .status()?;

        // Disable interactive post-install scripts for this command only
        let upgrade = Command::new("sh").arg("-c")
                                        .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} {}", snapshot,apt_upgrade))
                                        .env("DEBIAN_FRONTEND", "noninteractive")
                                        .status()?;
        if update.success() && upgrade.success() {
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
    let packages = "ca-certificates";
    let release = code_name(snapshot);
    let target_path = format!("/.snapshots/rootfs/snapshot-chr{}", snapshot);
    let bootstrap_cmd = format!("mmdebstrap --include='{}' {} {}", packages,release,target_path);

    // Bootstrap command
    let excode = Command::new("sh").arg("-c").arg(bootstrap_cmd).status()?;

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
        cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/var/cache/apt/archives/.", tmp),
                   format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/apt/archives", snapshot))?;
    } else {
        // Persist the chroot cache back to the deployed snapshot
        cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/apt/archives/.", snapshot),
                   format!("/.snapshots/rootfs/snapshot-{}/var/cache/apt/archives", tmp))?;
    }
    Ok(())
}

// Clean apt cache
pub fn clean_cache(snapshot: &str) -> Result<(), Error> {
    // Keep the archives directory itself: chr_delete() copies this cache back
    // to the deployed snapshot after cleanup, so removing the directory makes
    // that copy fail. Only clear the downloaded packages inside it.
    let archives = format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/apt/archives", snapshot);
    if Path::new(&archives).try_exists()? {
        remove_dir_content(&archives)?;
    }
    Ok(())
}

// Distribution code name
pub fn code_name(snapshot: &str) -> String {
    let mut code_name = String::new();
    // Check if /etc/lsb-release exists and contains DISTRIB_CODENAME
    if let Ok(file) = read_to_string(&format!("/.snapshots/rootfs/snapshot-{}/etc/lsb-release", snapshot)) {
        for line in file.lines() {
            if line.starts_with("DISTRIB_CODENAME") {
                if let Some((_, value)) = line.split_once('=') {
                    code_name = value.to_lowercase().trim_matches('"').to_string();
                    break;
                }
            }
        }
    }

    // If /etc/lsb-release check fails, check /etc/os-release for VERSION_CODENAME
    if code_name.is_empty() {
        if let Ok(file) = read_to_string(&format!("/.snapshots/rootfs/snapshot-{}/etc/os-release", snapshot)) {
            for line in file.lines() {
                if line.starts_with("VERSION_CODENAME") {
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

    // Run update before install
    let apt_update = "apt-get update";
    let apt_update = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} {}", snapshot,apt_update))
                                       .status()?;
    if !apt_update.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to run apt-get update"));
    }

    for pkg in pkgs {
        let mut pkgs_list: Vec<String> = Vec::new();
        if let Some(section) = profconf.get_map().and_then(|map| map.get("profile-packages").cloned()) {
            for pkg in section.keys() {
                pkgs_list.push(pkg.to_string());
            }
        }
        // Nocomfirm flag
        let install_args = if noconfirm {
            format!("apt-get install -y --no-install-recommends {}", pkg)
        } else {
            format!("apt-get install --no-install-recommends {}", pkg)
        };

        // Install packages using apt
        let apt_install = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} {}", snapshot,&install_args))
                                       .status()?;
        if !apt_install.success() {
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
    let pkg_list = format!("{pkgs:?}");
    let install_args = if noconfirm {
        let options = "-o Dpkg::Options::=\"--force-confdef\" -o Dpkg::Options::=\"--force-confold\"";
        format!("apt-get install {} -y --no-install-recommends {}", pkg_list.replace(&['[', ']', ',', '\"'][..], ""), options)
    } else {
        format!("apt-get install --no-install-recommends {}", pkg_list.replace(&['[', ']', ',', '\"'][..], ""))
    };

    // APT update
    let apt_update = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} apt-get update", snapshot))
                                       .status()?;
    if !apt_update.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to run apt-get update"));
    }

    // APT install
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} {}", snapshot,install_args));
    if noconfirm {
        cmd.env("DEBIAN_FRONTEND", "noninteractive");
    }
    let apt_install = cmd.status()?;
    if !apt_install.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to install package(s)."));
    }
    Ok(())
}

// Install atomic-operation in live snapshot
pub fn install_package_helper_live(_snapshot: &str, tmp: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    // APT update
    let apt_update = "apt-get update";
    let apt_update = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-{} {}", tmp,apt_update))
                                       .status()?;
    if !apt_update.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to run apt-get update")));
    }

    for pkg in pkgs {
        let install_args = if noconfirm {
            format!("apt-get install -y --no-install-recommends {}", pkg)
        } else {
            format!("apt-get install --no-install-recommends {}", pkg)
        };

        // APT install
        let apt_install = Command::new("sh").arg("-c")
                                            .arg(format!("chroot /.snapshots/rootfs/snapshot-{} {}", tmp,install_args))
                                            .status()?;

        if !apt_install.success() {
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

    // Drop the ineffective pin file left by previous versions:
    // Pin-Priority only steers version selection, apt-get remove and
    // autoremove still remove a pinned package
    let old_rule_file = format!("/.snapshots/rootfs/snapshot-chr{}/etc/apt/preferences.d/dsnap_system_packages", snapshot);
    if Path::new(&old_rule_file).try_exists()? {
        std::fs::remove_file(&old_rule_file)?;
    }

    // Release holds that are no longer in system-packages so the hold list
    // mirrors the profile exactly
    let held = Command::new("sh").arg("-c")
                                 .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} apt-mark showhold", snapshot))
                                 .output()?;
    if !held.status.success() {
        return Err(Error::new(ErrorKind::Other, "Failed to list held packages."));
    }
    for pkg in String::from_utf8_lossy(&held.stdout).lines() {
        let pkg = pkg.trim();
        if pkg.is_empty() || system_pkgs.iter().any(|s| s == pkg) {
            continue;
        }
        let excode = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} apt-mark unhold {}", snapshot, pkg))
                                       .status()?;
        if !excode.success() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to unhold package {}.", pkg)));
        }
    }

    // apt-mark hold is apt's native equivalent of pacman's HoldPkg and
    // dnf's protected.d: it prevents the package from being automatically
    // installed, upgraded or removed
    if !system_pkgs.is_empty() {
        let pkgs = system_pkgs.join(" ");
        let excode = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} apt-mark hold {}", snapshot, pkgs))
                                       .status()?;
        if !excode.success() {
            return Err(Error::new(ErrorKind::Other, "Failed to hold system packages."));
        }
    }
    Ok(())
}

fn parse_pkg_names(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|pkg| pkg.split(':').next().unwrap_or(pkg).to_string())
        .collect()
}

// Get list of manually installed packages, excluding dependency-only packages
pub fn no_dep_pkg_list(snapshot: &str, chr: &str) -> Result<Vec<String>, Error> {
    let root = format!("/.snapshots/rootfs/snapshot-{}{}", chr, snapshot);
    let excode = Command::new("chroot")
                          .arg(root)
                          .args(["apt-mark", "showmanual"])
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
                          .args(["dpkg-query", "-W", "-f=${Package}\n"])
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

// Run dpkg-query
pub fn pkg_query(pkg: &str) -> Result<String, Error> {
    let output = Command::new("dpkg-query")
                      .arg("-W")
                      .arg("-f=${Package} ${Version}")
                      .arg(pkg)
                      .output()?;
    if !output.status.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Package {} not found.", pkg)));
    }
    // dpkg-query prints "pkg version"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.split_whitespace().nth(1)
                        .ok_or_else(|| Error::new(ErrorKind::Other,
                                                   format!("Failed to parse version for package {}.", pkg)))?;
    Ok(version.to_string())
}

// Refresh snapshot atomic-operation
pub fn refresh_helper(snapshot: &str) -> Result<(), Error> {
    let refresh = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-chr{}", snapshot))
                                        .args(["apt-get", "update"])
                                        .status()?;
    if !refresh.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Refresh failed."));
    }
    Ok(())
}

// Disable service(s)
pub fn service_disable(snapshot: &str, services: &Vec<String>, chr: &str) -> Result<(), Error> {
    for service in services {
        // Systemd
        if has_systemd(snapshot, chr)? {
            let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-{}{}", chr,snapshot))
                                               .arg("systemctl")
                                               .arg("disable")
                                               .arg(&service).status()?;
            if !excode.success() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to disable {}.", service)));
            }
        } //TODO add more init system
    }
    Ok(())
}

// Enable service(s)
pub fn service_enable(snapshot: &str, services: &Vec<String>, chr: &str) -> Result<(), Error> {
    for service in services {
        // Systemd
        if has_systemd(snapshot, chr)? {
            let excode = Command::new("chroot").arg(format!("/.snapshots/rootfs/snapshot-{}{}", chr,snapshot))
                                               .arg("systemctl")
                                               .arg("enable")
                                               .arg(&service).status()?;
            if !excode.success() {
                return Err(Error::new(ErrorKind::Other,
                                      format!("Failed to enable {}.", service)));
            }
        } //TODO add more init
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

    // Copy apt configuration
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/etc/apt", snapshot))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/etc/apt/.", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/etc/apt/", snapshot))?;

    // Copy debconf
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/etc/debconf.conf", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/etc/debconf.conf", snapshot))?;
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/debconf", snapshot))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/var/cache/debconf/.", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/var/cache/debconf/", snapshot))?;
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/usr/share/debconf", snapshot))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/usr/share/debconf/.", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/usr/share/debconf/", snapshot))?;

    // Copy keyrings directory
    remove_dir_content(&format!("/.snapshots/rootfs/snapshot-chr{}/usr/share/keyrings", snapshot))?;
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/usr/share/keyrings/.", snapshot),
               format!("/.snapshots/rootfs/snapshot-chr{}/usr/share/keyrings/", snapshot))?;

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
    cp_reflink_no_clobber(format!("/.snapshots/rootfs/snapshot-{}/var/cache/apt/archives/.", s_f),
               format!("/.snapshots/rootfs/snapshot-{}{}/var/cache/apt/archives", chr,s_t))?;
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
            vec!["apt-get", "remove", "-y"]
        } else {
            vec!["apt-get", "remove"]
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
    for pkg in pkgs {
        let uninstall_args = if noconfirm {
            "apt-get remove -y"
        } else {
            "apt-get remove"
        };

        let excode = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} {} {}", snapshot,uninstall_args, pkg)).status()?;

        if !excode.success() {
            return Err(Error::new(ErrorKind::Other,
                                  format!("Failed to uninstall {}", pkg)));
        }
    }
    Ok(())
}

// Uninstall package(s) atomic-operation live snapshot
pub fn uninstall_package_helper_live(tmp: &str, pkgs: &Vec<String>, noconfirm: bool) -> Result<(), Error> {
    for pkg in pkgs {
        let uninstall_args: Vec<&str> = if noconfirm {
            vec!["apt-get", "remove", "-y"]
        } else {
            vec!["apt-get", "remove"]
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

    let upgrade_args = if noconfirm {
        // Avoid interactive conffile prompts during noninteractive upgrades
        "env DEBIAN_FRONTEND=noninteractive apt-get -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold upgrade"
    } else {
        "apt-get upgrade"
    };

    // Run apt update
    let apt_update = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} apt-get update", snapshot))
                                       .status()?;
    if !apt_update.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to run apt-get update"));
    }

    // Run apt upgrade
    let apt_upgrade = Command::new("sh").arg("-c")
                                        .arg(format!("chroot /.snapshots/rootfs/snapshot-chr{} {}", snapshot,upgrade_args))
                                        .status()?;
    if !apt_upgrade.success() {
        return Err(Error::new(ErrorKind::Other,
                              format!("Failed to upgrade snapshot {}.", snapshot)));
    }
    Ok(())
}

// Live upgrade snapshot atomic-operation
pub fn upgrade_helper_live(tmp: &str, noconfirm: bool) -> Result<(), Error> {
    let upgrade_args = if noconfirm {
        // Avoid interactive conffile prompts during noninteractive upgrades
        "env DEBIAN_FRONTEND=noninteractive apt-get -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold upgrade"
    } else {
        "apt-get upgrade"
    };

    // Run apt update
    let apt_update = Command::new("sh").arg("-c")
                                       .arg(format!("chroot /.snapshots/rootfs/snapshot-{} apt-get update", tmp))
                                       .status()?;
    if !apt_update.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to run apt-get update"));
    }

    // Run apt upgrade
    let excode = Command::new("sh").arg("-c")
                                   .arg(format!("chroot /.snapshots/rootfs/snapshot-{} {}", tmp,upgrade_args))
                                   .status()?;
    if !excode.success() {
        return Err(Error::new(ErrorKind::Other,
                              "Failed to upgrade current/live snapshot."));
    }
    Ok(())
}
