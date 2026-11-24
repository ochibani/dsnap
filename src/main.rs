extern crate lib;
#[cfg(feature = "btrfs")]
pub mod btrfs;
mod cli;

use cli::*;
use lib::*;
use nix::unistd::Uid;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

static FAILED: AtomicBool = AtomicBool::new(false);

macro_rules! fail {
    ($($arg:tt)*) => {{
        ::std::eprintln!($($arg)*);
        FAILED.store(true, Ordering::SeqCst);
    }};
}

fn failure() -> ExitCode {
    FAILED.store(true, Ordering::SeqCst);
    ExitCode::FAILURE
}

fn audit_cli_invocation(subcommand: &str) {
    let argv = format!("{:?}", std::env::args().skip(1).collect::<Vec<_>>());
    audit_log(subcommand, &[("argv", argv.as_str())]);
}

// Select Bootloader
#[cfg(feature = "grub")]
pub mod grub;
#[cfg(feature = "grub")]
use grub::update_boot;
//TODO add systemd-boot


// Directories
// Global boot is always at @boot_linux
// *-chr                             : temporary directories used to chroot into snapshot or copy snapshots around
// *-deploy and *-deploy-aux         : temporary directories used to boot deployed snapshot
// *-deploy[-aux]-secondary          : temporary directories used to boot secondary deployed snapshot
// *-recovery-deploy[-aux]           : temporary directories used to boot deployed recovery snapshot
// /.snapshots/dsnap/deploy-tmp        : deployed snapshots temporary boot directories
// /.snapshots/dsnap/export            : default export path
// /.snapshots/dsnap/part              : root partition uuid
// /.snapshots/dsnap/snapshots/*-desc  : snapshots descriptions
// /.snapshots/dsnap/upstate           : state of last system update
// /.snapshots/boot/boot-*           : individual /boot for each snapshot
// /.snapshots/etc/etc-*             : individual /etc for each snapshot
// /.snapshots/var/var-*             : individual /var for each snapshot
// /.snapshots/rootfs/snapshot-*     : snapshots
// /.snapshots/tmp                   : temporary directory
// /etc/dsnap/dsnap.conf                 : configuration file for dsnap
// /etc/dsnap/profile                  : snapshot profile
// /usr/bin/dsnap                      : dsnap binary file location
// /usr/share/dsnap                    : files that store current snapshot info
// /usr/share/dsnap/profiles           : default desktop environments profiles path
// /usr/share/dsnap/rec-tmp            : name of temporary directory used to boot recovery snapshot
// /usr/share/dsnap/snap               : snapshot number
// /var/lib/dsnap(/fstree)             : dsnap files, stores fstree, symlink to /.snapshots/dsnap/fstree

fn main() -> ExitCode {
    if !Uid::effective().is_root() {
        fail!("sudo/doas is required to run dsnap!");
    } else {
        match chroot_check() {
            Ok(true) => fail!("Please don't use dsnap inside a chroot!"),
            Err(e) => fail!("Failed to determine chroot state: {}", e),
            Ok(false) => {
        // Call cli matches
        let matches = cli().get_matches();
        // Keep informational commands quiet; only audit and warn before
        // operations that can change system or boot state.
        let subcommand = matches.subcommand_name().unwrap_or("unknown");
        if !matches!(matches.subcommand_name(),
                     None | Some("current" | "difference" | "last-update" | "list"
                                 | "subvolumes" | "tree" | "version" | "whichsnap" | "whichtmp")) {
            audit_cli_invocation(subcommand);
            print_boot_security_warnings();
        }
        // Call relevant functions
        match matches.subcommand() {
            // Auto upgrade
            Some(("auto-upgrade", auto_upgrade_matches)) => {
                // Get snapshot value
                let snapshot = if auto_upgrade_matches.contains_id("SNAPSHOT") {
                    let snap = auto_upgrade_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run noninteractive_update
                let run = noninteractive_update(&snapshot);
                match run {
                    Ok(_) => println!("The auto-upgrade has been completed successfully."),
                    Err(e) => fail!("{}", e),
                }
            }
            // Base import
            Some(("base-import", base_import_matches)) => {
                // Get user_profile value
                let path: String = base_import_matches.get_many::<String>("SNAPSHOT_PATH").unwrap().map(|s| format!("{}", s)).collect();

                // Get tmp_dir value
                let tmp_dir = match tempfile::TempDir::new_in("/.snapshots/tmp") {
                    Ok(dir) => dir,
                    Err(e) => {
                        fail!("Failed to create temporary directory: {}", e);
                        return failure();
                    }
                };

                // Run import_base
                let allow_unverified = base_import_matches.get_flag("ALLOW_UNVERIFIED");
                let run = import_base(&path, &tmp_dir, allow_unverified);
                if cfg!(feature = "base-import") {
                    match run {
                        Ok(_) => {
                            if let Err(e) = post_transactions("0") {
                                // Clean chroot mount directories
                                if let Err(clean_err) = chr_delete("0") {
                                    fail!("Failed to clean chroot after base import failure: {}", clean_err);
                                }
                                fail!("Failed to finish base import: {}", e);
                            } else {
                                println!("New base snapshot has been successfully imported.")
                            }
                        },
                        Err(e) => {
                            // Clean chroot mount directories
                            if let Err(clean_err) = chr_delete("0") {
                                fail!("Failed to clean chroot after base import failure: {}", clean_err);
                            }
                            fail!("{}", e);
                        },
                    }
                } else {
                    fail!("base-import subcommand is not supported.");
                }
            }
            // Base rebuild
            Some(("base-rebuild", _matches)) => {
                // Run rebuild_base
                let run = rebuild_base();
                match run {
                    Ok(_) => println!("Base snapshot was rebuilt successfully."),
                    Err(e) => fail!("{}", e),
                }
            }
            // Base update
            Some(("base-update", base_update_matches)) => {
                // Optional value
                let noconfirm = base_update_matches.get_flag("noconfirm");

                // Run upgrade(0)
                let run = upgrade("0", true, noconfirm);
                match run {
                    Ok(_) => {},
                    Err(e) => fail!("{}", e),
                }
            }
            // Boot update command
            Some(("boot-update", boot_matches)) => {
                // Get snapshot value
                let snapshot = if boot_matches.contains_id("SNAPSHOT") {
                    let snap = boot_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run update_boot
                let run = update_boot(&snapshot, false);
                match run {
                    Ok(_) => println!("Bootloader updated successfully."),
                    Err(e) => fail!("{}", e),
                }
            }
            // Branch
            Some(("add-branch", branch_matches)) => {
                // Get snapshot value
                let snapshot = if branch_matches.contains_id("SNAPSHOT") {
                    let snap = branch_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get desc value
                let desc = if branch_matches.contains_id("DESCRIPTION") {
                    let desc = branch_matches.get_one::<String>("DESCRIPTION").map(|s| s.as_str()).unwrap().to_string();
                    desc
                } else {
                    let desc = String::new();
                    desc
                };

                // Run barnch_create
                let run = branch_create(&snapshot, &desc);
                match run {
                    Ok(snapshot_num) => println!("Branch {} added under snapshot {}.", snapshot_num,snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Check last update status
            Some(("last-update", _matches)) => {
                // Run check_update
                let run = check_update();
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Chroot
            Some(("chroot", chroot_matches)) => {
                // Get snapshot value
                let snapshot  = if chroot_matches.contains_id("SNAPSHOT") {
                    let snap = chroot_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get cmd value
                let cmd: Vec<String> = Vec::new();

                // Run chroot
                let run = chroot(&snapshot, cmd);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Clone
            Some(("clone", clone_matches)) => {
                // Get snapshot value
                let snapshot = if clone_matches.contains_id("SNAPSHOT") {
                    let snap = clone_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get desc value
                let desc = if clone_matches.contains_id("DESCRIPTION") {
                    let desc = clone_matches.get_one::<String>("DESCRIPTION").map(|s| s.as_str()).unwrap().to_string();
                    desc
                } else {
                    let desc = String::new();
                    desc
                };

                // Run clone_as_tree
                let run = clone_as_tree(&snapshot, &desc);
                match run {
                    Ok(snapshot_num) => println!("Tree {} cloned from {}.", snapshot_num,snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Clone a branch
            Some(("clone-branch", clone_branch_matches)) => {
                // Get snapshot value
                let snapshot = if clone_branch_matches.contains_id("SNAPSHOT") {
                    let snap = clone_branch_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run clone_branch
                let run = clone_branch(&snapshot);
                match run {
                    Ok(snapshot_num) => println!("Branch {} added to parent of {}.", snapshot_num,snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Clone recursively
            Some(("clone-tree", clone_tree_matches)) => {
                // Get snapshot value
                let snapshot = if clone_tree_matches.contains_id("SNAPSHOT") {
                    let snap = clone_tree_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run clone_recursive
                let run = clone_recursive(&snapshot);
                match run {
                    Ok(_) => println!("Snapshot {} was cloned recursively.", snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Clone under a branch
            Some(("clone-under", clone_under_matches)) => {
                // Get snapshot value
                let snap = clone_under_matches.get_one::<i32>("SNAPSHOT").unwrap();
                let snapshot = format!("{}", snap);

                // Get branch value
                let branch_i32 = clone_under_matches.get_one::<i32>("BRANCH").unwrap();
                let branch = format!("{}", branch_i32);

                // Run clone_under
                let run = clone_under(&snapshot, &branch);
                match run {
                    Ok(snapshot_num) => println!("Branch {} added under snapshot {}.", snapshot_num,snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Current snapshot
            Some(("current", _matches)) => {
                // Run get_current_snapshot
                match get_current_snapshot() {
                    Ok(snap) => println!("{}", snap),
                    Err(e) => fail!("{}", e),
                }
            }
            // Delete
            Some(("delete", del_matches)) => {
                // Get snapshot value
                let snapshots: Vec<_> = del_matches.get_many::<i32>("SNAPSHOT").unwrap().map(|s| format!("{}", s)).collect();

                // Delete options
                let quiet = del_matches.get_flag("quiet");
                let nuke = del_matches.get_flag("nuke");

                // Run delelte_node
                let run = delete_node(&snapshots, quiet, nuke);
                match run {
                    Ok(_) => println!("Snapshot(s) {:?} removed.", snapshots),
                    Err(e) => fail!("{}", e),
                }
            }
            // Deploy
            Some(("deploy", deploy_matches)) => {
                // Get snapshot value
                let snapshot = if deploy_matches.contains_id("SNAPSHOT") {
                    let snap = deploy_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Optional value
                let secondary = deploy_matches.get_flag("secondary");

                // Run deploy
                let run = deploy(&snapshot, secondary, false);
                match run {
                    Ok(target_dep) => {
                        // Save deployed snapshots tmp
                        let source_dep = get_tmp();
                        match OpenOptions::new().truncate(true)
                                                 .create(true)
                                                 .read(true)
                                                 .write(true)
                                                 .open("/.snapshots/dsnap/deploy-tmp") {
                            Ok(mut deploy_tmp) => {
                                let tmp = format!("{}\n{}", source_dep,target_dep);
                                if let Err(e) = deploy_tmp.write_all(tmp.as_bytes()) {
                                    fail!("Failed to record deployed snapshot: {}", e);
                                    return failure();
                                }
                                println!("Snapshot {} deployed to '/'.", snapshot);
                            }
                            Err(e) => {
                                fail!("Failed to open deployment state file: {}", e);
                                return failure();
                            }
                        }
                    },
                    Err(e) => fail!("{}", e),
                }
            }
            // Description
            Some(("description", desc_matches)) => {
                // Get snapshot value
                let snapshot = if desc_matches.contains_id("SNAPSHOT") {
                    let snap = desc_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get desc value
                let desc = desc_matches.get_one::<String>("DESCRIPTION").map(|s| s.as_str()).unwrap().to_string();

                // Run write_desc
                let run = write_desc(&snapshot, &desc, true);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Diff two snapshots
            Some(("difference", diff_matches)) => {
                // Get snapshot one value
                let snap1 = diff_matches.get_one::<i32>("SNAPSHOT-1").unwrap();
                let snapshot1 = format!("{}", snap1);

                // Get snapshot two value
                let snap2 = diff_matches.get_one::<i32>("SNAPSHOT-2").unwrap();
                let snapshot2 = format!("{}", snap2);

                // Run diff
                match diff(&snapshot1, &snapshot2) {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Edit per-snapshot dsnap configuration
            Some(("edit-config", edit_config_matches)) => {
                // Get snapshot value
                let snapshot = if edit_config_matches.contains_id("SNAPSHOT") {
                    let snap = edit_config_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run snapshot_config_edit (edits dsnap.conf)
                let run = snapshot_config_edit(&snapshot);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Edit snapshot profile
            Some(("edit-profile", edit_profile_matches)) => {
                // Get snapshot value
                let snapshot = if edit_profile_matches.contains_id("SNAPSHOT") {
                    let snap = edit_profile_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run snapshot_profile_edit
                let run = snapshot_profile_edit(&snapshot);
                match run {
                    Ok(_) => {
                        println!("snapshot {} profile has been successfully updated.", &snapshot)
                    },
                    Err(e) => {
                        let _ = chr_delete(&snapshot);
                        fail!("{}", e);
                    },
                }
            }
            // Switch distros
            Some(("efi-update", _matches)) => {
                // Run efi_boot_order
                if is_efi() {
                    let run = efi_boot_order();
                    match run {
                        Ok(_) => println!("EFI boot order has been updated."),
                        Err(e) => fail!("{}", e),
                    }
                } else {
                   fail!("efi-update command is not supported.");
                }
            }
            // etc update
            Some(("etc-update", _matches)) => {
                // Run update_etc
                let run = update_etc();
                match run {
                    Ok(_) => println!("etc has been successfully updated."),
                    Err(e) => fail!("{}", e),
                }
            }
            // Export snapshot
            Some(("export", export_matches)) => {
                // Get snapshot value
                let snapshot = if export_matches.contains_id("SNAPSHOT") {
                    let snap = export_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get dest value
                let dest = if export_matches.contains_id("OUTPUT") {
                    let dest = export_matches.get_one::<String>("OUTPUT").map(|s| s.as_str()).unwrap().to_string();
                    dest
                } else {
                    let dest = String::from("/.snapshots/dsnap/export");
                    dest
                };

                // Get signing key value
                let signing_key = export_matches.get_one::<String>("SIGNING_KEY")
                                               .map(|s| s.as_str())
                                               .unwrap()
                                               .to_string();

                // Run export
                if !Path::new("/.snapshots/dsnap/export").try_exists().unwrap_or(false) {
                    match Command::new("mkdir").arg("-p")
                                               .arg("/.snapshots/dsnap/export")
                                               .status() {
                        Ok(status) if status.success() => {}
                        Ok(status) => {
                            fail!("Failed to create export directory (exit status: {}).", status);
                            return failure();
                        }
                        Err(e) => {
                            fail!("Failed to create export directory: {}", e);
                            return failure();
                        }
                    }
                }

                // Run export
                let run = export(&snapshot, &dest, &signing_key);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Fix db commands
            Some(("fixdb", fixdb_matches)) => {
                // Get snapshot value
                let snapshot = if fixdb_matches.contains_id("SNAPSHOT") {
                    let snap = fixdb_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    let snap = String::new();
                    snap
                };

                //Run fixdb
                let run = fixdb(&snapshot);
                if cfg!(feature = "pacman") {
                    match run {
                        Ok(_) => {
                            let snapshot = if snapshot.is_empty() {
                                match get_current_snapshot() {
                                    Ok(snap) => snap,
                                    Err(e) => {
                                        fail!("Failed to determine current snapshot: {}", e);
                                        return failure();
                                    }
                                }
                            } else {
                                snapshot.clone()
                            };
                            match post_transactions(&snapshot) {
                                Ok(_) => println!("Snapshot {}'s package manager database fixed successfully.", snapshot),
                                Err(e) => fail!("Failed to finish fixing package manager database: {}", e),
                            }
                        },
                        Err(e) => {
                            let snapshot = if snapshot.is_empty() {
                                match get_current_snapshot() {
                                    Ok(snap) => snap,
                                    Err(e) => {
                                        fail!("Failed to determine current snapshot: {}", e);
                                        return failure();
                                    }
                                }
                            } else {
                                snapshot.clone()
                            };
                            if let Err(clean_err) = chr_delete(&snapshot) {
                                fail!("Failed to clean chroot after fixdb failure: {}", clean_err);
                            }
                            fail!("{}", e);
                        },
                    }
                } else {
                    fail!("fixdb subcommand is not supported.");
                }
            }
            // Hollow a node
            Some(("hollow", hollow_matches)) => {
                // Get snapshot value
                let snapshot = if hollow_matches.contains_id("SNAPSHOT") {
                    let snap = hollow_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run hollow
                let run = hollow(&snapshot);
                match run {
                    Ok(_) => println!("Snapshot {} hollow operation succeeded. Please reboot!", snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Immutability disable
            Some(("immutable-disable", immdis_matches)) => {
                // Get snapshot value
                let snapshot = if immdis_matches.contains_id("SNAPSHOT") {
                    let snap = immdis_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run immutability_disable
                let run = immutability_disable(&snapshot);
                match run {
                    Ok(_) => println!("Snapshot {} successfully made mutable.", snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Immutability enable
            Some(("immutable-enable", immen_matches)) => {
                // Get snapshot value
                let snapshot = if immen_matches.contains_id("SNAPSHOT") {
                    let snap = immen_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run immutability_enable
                let run = immutability_enable(&snapshot);
                match run {
                    Ok(_) => println!("Snapshot {} successfully made immutable.", snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Import snapshot
            Some(("import", import_matches)) => {
                // Get snapshot value
                let snapshot = match find_new() {
                    Ok(snap) => snap,
                    Err(e) => {
                        fail!("Failed to determine next snapshot number: {}", e);
                        return failure();
                    }
                };

                // Get desc value
                let desc = if import_matches.contains_id("DESCRIPTION") {
                    let desc = import_matches.get_one::<String>("DESCRIPTION").map(|s| s.as_str()).unwrap().to_string();
                    desc
                } else {
                    let desc = String::new();
                    desc
                };

                // Get user_profile value
                let path: String = import_matches.get_many::<String>("SNAPSHOT_PATH").unwrap().map(|s| format!("{}", s)).collect();

                // Get tmp_dir value
                let tmp_dir = match tempfile::TempDir::new_in("/.snapshots/tmp") {
                    Ok(dir) => dir,
                    Err(e) => {
                        fail!("Failed to create temporary directory: {}", e);
                        return failure();
                    }
                };

                // Run import
                let allow_unverified = import_matches.get_flag("ALLOW_UNVERIFIED");
                let run = import(snapshot, &path, &desc, &tmp_dir, allow_unverified);
                if cfg!(feature = "import") {
                    match run {
                        Ok(_) => {
                            println!("Snapshot {} has been successfully imported.", snapshot);
                        },
                        Err(e) => {
                            // Best-effort cleanup of a received snapshot left in the temp directory
                            let tmp_path = tmp_dir.path().to_string_lossy().to_string();
                            let received = format!("{}/{}", tmp_path, snapshot);
                            if Path::new(&received).try_exists().unwrap_or(false) {
                                #[cfg(feature = "btrfs")]
                                if let Err(clean_err) = btrfs::delete_subvolume(&received) {
                                    fail!("Failed to clean temporary received snapshot: {}", clean_err);
                                }
                            }
                            // Clean chroot mount directories
                            if let Err(clean_err) = chr_delete(&format!("{}", snapshot)) {
                                fail!("Failed to clean chroot after import failure: {}", clean_err);
                            }
                            fail!("{}", e);
                        },
                    }
                } else {
                    fail!("import subcommand is not supported.");
                }
            }
            // Install command
            Some(("install", install_matches)) => {
                // Get snapshot value
                let snapshot = if install_matches.contains_id("SNAPSHOT") {
                    let snap = install_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get pkgs value
                let pkgs = if install_matches.contains_id("PACKAGE") {
                    let pkgs: Vec<String> = install_matches.get_many::<String>("PACKAGE").unwrap().map(|s| format!("{}", s)).collect();
                    pkgs
                } else {
                    let pkgs: Vec<String> = Vec::new();
                    pkgs
                };

                // Get profile value
                let profile = if install_matches.contains_id("PROFILE") {
                    let profile = install_matches.get_many::<String>("PROFILE").unwrap().map(|s| format!("{}", s)).collect();
                    profile
                } else {
                    let profile = String::new();
                    profile
                };

                // Get user_profile value
                let user_profile = if install_matches.contains_id("USER_PROFILE") {
                    let user_profile = install_matches.get_many::<String>("USER_PROFILE").unwrap().map(|s| format!("{}", s)).collect();
                    user_profile
                } else {
                    let user_profile = String::new();
                    user_profile
                };

                // Install options
                let live = install_matches.get_flag("live");
                let noconfirm = install_matches.get_flag("noconfirm");
                let force = install_matches.get_flag("force");
                let secondary = install_matches.get_flag("secondary");

                // Run install_triage
                let run = install_triage(&snapshot, live, pkgs, &profile, force, &user_profile, noconfirm, secondary);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Package list
            Some(("list", list_matches)) => {
                // Get snapshot value
                let snapshot = if list_matches.contains_id("SNAPSHOT") {
                    let snap = list_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // chr value
                let chr = "chr";

                // Optional value
                let exclude = list_matches.get_flag("exclude-dependency");

                // Make sure snapshot exists
                match Path::new(&format!("/.snapshots/rootfs/snapshot-{}", snapshot)).try_exists() {
                    Ok(false) => {
                        fail!("Cannot list packages as snapshot {} doesn't exist.", snapshot);
                    }
                    Err(e) => {
                        fail!("Failed to check snapshot {}: {}", snapshot, e);
                    }
                    Ok(true) => {
                        match Path::new(&format!("/.snapshots/rootfs/snapshot-chr{}", snapshot)).try_exists() {
                            Ok(true) => {
                                // Make sure snapshot is not in use by another dsnap process
                                fail!("Snapshot {} appears to be in use. If you're certain it's not in use, clear lock with 'dsnap unlock -s {}'.",
                                          snapshot,snapshot);
                            }
                            Err(e) => {
                                fail!("Failed to check snapshot {} lock state: {}", snapshot, e);
                            }
                            Ok(false) => {
                                // Run list
                                match list(&snapshot, chr, exclude) {
                                    Ok(pkgs) => {
                                        for pkg in pkgs {
                                            println!("{}", pkg);
                                        }
                                    }
                                    Err(e) => fail!("{}", e),
                                }
                            }
                        }
                    }
                }
            }
            // Live chroot
            Some(("live-chroot", _matches)) => {
                // Run live_unlock
                let run = live_unlock();
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // New
            Some(("new-tree", new_matches)) => {
                // Get desc value
                let desc = if new_matches.contains_id("DESCRIPTION") {
                    let desc = new_matches.get_one::<String>("DESCRIPTION").map(|s| s.as_str()).unwrap().to_string();
                    desc
                } else {
                    let desc = String::new();
                    desc
                };

                // Run snapshot_base_new
                let run = snapshot_base_new(&desc);
                match run {
                    Ok(snap_num) => println!("New tree {} created.", snap_num),
                    Err(e) => fail!("{}", e),
                }
            }
            // Rebuild
            Some(("rebuild", rebuild_matches)) => {
                // Get snapshot value
                let snapshot = if rebuild_matches.contains_id("SNAPSHOT") {
                    let snap = rebuild_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get desc value
                let desc = if rebuild_matches.contains_id("DESCRIPTION") {
                    let desc = rebuild_matches.get_one::<String>("DESCRIPTION").map(|s| s.as_str()).unwrap().to_string();
                    desc
                } else {
                    let desc = String::new();
                    desc
                };

                // Get new snapshot number
                let snap_num = match find_new() {
                    Ok(n) => n,
                    Err(e) => {
                        fail!("Failed to determine next snapshot number: {}", e);
                        return failure();
                    }
                };

                // Run rebuild
                let run = rebuild(&snapshot, snap_num, &desc);
                match run {
                    Ok(snap_num) => {
                        println!("Tree {} cloned from {}.", snap_num,snapshot);
                    },
                    Err(e) => {
                        fail!("{}", e);
                    },
                }
            }
            // Refresh
            Some(("refresh", refresh_matches)) => {
                // Get snapshot value
                let snapshot = if refresh_matches.contains_id("SNAPSHOT") {
                    let snap = refresh_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run refresh
                let run = refresh(&snapshot);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Reset
            Some(("reset", _matches)) => {
                // Run reset
                let run = reset();
                match run {
                    Ok(_) => {
                        // Reboot system
                        match Command::new("reboot").status() {
                            Ok(status) if status.success() => {}
                            Ok(status) => fail!("Failed to reboot (exit status: {}).", status),
                            Err(e) => fail!("Failed to reboot: {}", e),
                        }
                    },
                    Err(e) => fail!("{}", e),
                }
            }
            // Rollback
            Some(("rollback", _matches)) => {
                // Run rollback
                let run = rollback();
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Chroot run
            Some(("run", run_matches)) => {
                // Get snapshot value
                let snapshot = if run_matches.contains_id("SNAPSHOT") {
                    let snap = run_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get cmds value
                let cmds: Vec<String> = run_matches.get_many::<String>("COMMAND").unwrap().map(|s| format!("{}", s)).collect();

                // Run chroot
                let run = chroot(&snapshot, cmds);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Subvolumes list
            Some(("subvolumes", _matches)) => {
                // Run list_subvolumes
                if let Err(e) = list_subvolumes() {
                    fail!("{}", e);
                }
            }
            // Tree sync
            Some(("tree-sync", sync_matches)) => {
                // Get treename value
                let treename = if sync_matches.contains_id("TREENAME") {
                    let snap = sync_matches.get_one::<i32>("TREENAME").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Tree sync options
                let live = sync_matches.get_flag("live");

                // Run tree_sync
                let run = tree_sync(&treename, live);
                match run {
                    Ok(_) => println!("Tree {} synced.", treename),
                    Err(e) => fail!("{}", e),
                }
            }
            // Tree install
            Some(("tree-install", tinstall_matches)) => {
                // Get treename value
                let treename = if tinstall_matches.contains_id("TREENAME") {
                    let snap = tinstall_matches.get_one::<i32>("TREENAME").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get pkgs value
                let pkgs = if tinstall_matches.contains_id("PACKAGE") {
                    let pkgs: Vec<String> = tinstall_matches.get_many::<String>("PACKAGE").unwrap().map(|s| format!("{}", s)).collect();
                    pkgs
                } else {
                    let pkgs: Vec<String> = Vec::new();
                    pkgs
                };

                // Get profiles value
                let profiles = if tinstall_matches.contains_id("PROFILE") {
                    let profiles: Vec<String> = tinstall_matches.get_many::<String>("PROFILE").unwrap().map(|s| format!("{}", s)).collect();
                    profiles
                } else {
                    let profiles: Vec<String> = Vec::new();
                    profiles
                };

                // Get user_profiles value
                let user_profiles = if tinstall_matches.contains_id("USER_PROFILE") {
                    let user_profiles: Vec<String> = tinstall_matches.get_many::<String>("USER_PROFILE").unwrap().map(|s| format!("{}", s)).collect();
                    user_profiles
                } else {
                    let user_profiles: Vec<String> = Vec::new();
                    user_profiles
                };

                // Tree install options
                let noconfirm = tinstall_matches.get_flag("noconfirm");
                let force = tinstall_matches.get_flag("force");
                let secondary = tinstall_matches.get_flag("secondary");

                // Run tree_install
                let run = tree_install(&treename, &pkgs, &profiles, force, &user_profiles, noconfirm, secondary);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // tmp (clear tmp)
            Some(("clean-tmp", _matches)) => {
                // Run temp_snapshots_clear
                let run = temp_snapshots_clear();
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Tree
            Some(("tree", _matches)) => {
                //Run tree_show
                if let Err(e) = tree_show() {
                    fail!("{}", e);
                }
            }
            // Tree remove
            Some(("tree-uninstall", tremove_matches)) => {
                // Get treename value
                let treename = if tremove_matches.contains_id("TREENAME") {
                    let snap = tremove_matches.get_one::<i32>("TREENAME").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get pkgs value
                let pkgs = if tremove_matches.contains_id("PACKAGE") {
                    let pkgs: Vec<String> = tremove_matches.get_many::<String>("PACKAGE").unwrap().map(|s| format!("{}", s)).collect();
                    pkgs
                } else {
                    let pkgs: Vec<String> = Vec::new();
                    pkgs
                };

                // Get profiles value
                let profiles = if tremove_matches.contains_id("PROFILE") {
                    let profiles: Vec<String> = tremove_matches.get_many::<String>("PROFILE").unwrap().map(|s| format!("{}", s)).collect();
                    profiles
                } else {
                    let profiles: Vec<String> = Vec::new();
                    profiles
                };

                // Get user_profiles value
                let user_profiles = if tremove_matches.contains_id("USER_PROFILE") {
                    let user_profiles: Vec<String> = tremove_matches.get_many::<String>("USER_PROFILE").unwrap().map(|s| format!("{}", s)).collect();
                    user_profiles
                } else {
                    let user_profiles: Vec<String> = Vec::new();
                    user_profiles
                };

                // Optional value
                let noconfirm = tremove_matches.get_flag("noconfirm");

                // Run tree_remove
                let run = tree_remove(&treename, &pkgs, &profiles, &user_profiles, noconfirm);
                match run {
                    Ok(_) => println!("Tree {} updated.", treename),
                    Err(e) => fail!("{}", e),
                }
            }
            // Tree run
            Some(("tree-run", trun_matches)) => {
                // Get snapshot value
                let treename = if trun_matches.contains_id("TREENAME") {
                    let snap = trun_matches.get_one::<i32>("TREENAME").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(treename) => treename,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get cmds value
                let cmds: Vec<String> = trun_matches.get_many::<String>("COMMAND").unwrap().map(|s| format!("{}", s)).collect();

                // Run tree_run
                for cmd in cmds {
                    let run = tree_run(&treename, &cmd);
                    match run {
                        Ok(_) => println!("Tree {} updated.", treename),
                        Err(e) => fail!("{}", e),
                    }
                }
            }
            // Tree upgrade
            Some(("tree-upgrade", tupgrade_matches)) => {
                // Get treename value
                let treename = if tupgrade_matches.contains_id("TREENAME") {
                    let snap = tupgrade_matches.get_one::<i32>("TREENAME").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run tree_upgrade
                let run = tree_upgrade(&treename);
                match run {
                    Ok(_) => println!("Tree {} updated.", treename),
                    Err(e) => fail!("{}", e),
                }
            }
            // Uninstall package(s) from a snapshot
            Some(("uninstall", uninstall_matches)) => {
                // Get snapshot value
                let snapshot = if uninstall_matches.contains_id("SNAPSHOT") {
                    let snap = uninstall_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Get pkgs value
                let pkgs = if uninstall_matches.contains_id("PACKAGE") {
                    let pkgs: Vec<String> = uninstall_matches.get_many::<String>("PACKAGE").unwrap().map(|s| format!("{}", s)).collect();
                    pkgs
                } else {
                    let pkgs: Vec<String> = Vec::new();
                    pkgs
                };

                // Get profile value
                let profile = if uninstall_matches.contains_id("PROFILE") {
                    let profile = uninstall_matches.get_many::<String>("PROFILE").unwrap().map(|s| format!("{}", s)).collect();
                    profile
                } else {
                    let profile = String::new();
                    profile
                };

                // Get user_profile value
                let user_profile = if uninstall_matches.contains_id("USER_PROFILE") {
                    let user_profile = uninstall_matches.get_many::<String>("USER_PROFILE").unwrap().map(|s| format!("{}", s)).collect();
                    user_profile
                } else {
                    let user_profile = String::new();
                    user_profile
                };

                // Uninstall options
                let live = uninstall_matches.get_flag("live");
                let noconfirm = uninstall_matches.get_flag("noconfirm");

                // Run uninstall_triage
                let run = uninstall_triage(&snapshot, live, pkgs, &profile, &user_profile, noconfirm);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Service control
            Some(("service", service_matches)) => {
                let snapshot = if service_matches.contains_id("SNAPSHOT") {
                    let snap = service_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    format!("{}", snap)
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => { fail!("{}", e); return failure(); }
                    }
                };
                let enable: Vec<String> = if service_matches.contains_id("ENABLE") {
                    service_matches.get_many::<String>("ENABLE").unwrap().map(|s| s.to_string()).collect()
                } else { Vec::new() };
                let disable: Vec<String> = if service_matches.contains_id("DISABLE") {
                    service_matches.get_many::<String>("DISABLE").unwrap().map(|s| s.to_string()).collect()
                } else { Vec::new() };

                match service_control(&snapshot, &enable, &disable) {
                    Ok(_) if enable.is_empty() && disable.is_empty() => {}
                    Ok(_) => println!("Service state updated in snapshot {}.", snapshot),
                    Err(e) => fail!("{}", e),
                }
            }
            // Unlock a snapshot
            Some(("unlock", unlock_matches)) => {
                // Get snapshot value
                let snapshot = if unlock_matches.contains_id("SNAPSHOT") {
                    let snap = unlock_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Run snapshot_unlock
                let run = snapshot_unlock(&snapshot);
                match run {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Upgrade a snapshot
            Some(("upgrade", upgrade_matches)) => {
                // Get snapshot value
                let snapshot = if upgrade_matches.contains_id("SNAPSHOT") {
                    let snap = upgrade_matches.get_one::<i32>("SNAPSHOT").unwrap();
                    let snap_to_string = format!("{}", snap);
                    snap_to_string
                } else {
                    match get_current_snapshot() {
                        Ok(snap) => snap,
                        Err(e) => {
                            fail!("{}", e);
                            return failure();
                        }
                    }
                };

                // Upgrade options
                let noconfirm = upgrade_matches.get_flag("noconfirm");
                let live = upgrade_matches.get_flag("live");

                // Run upgrade
                let run = if live {
                    upgrade_live(&snapshot, noconfirm)
                } else {
                    upgrade(&snapshot, false, noconfirm)
                };
                match run {
                    Ok(_) => {},
                    Err(e) => fail!("{}", e),
                }
            }
            // dsnap version
            Some(("version", _matches)) => {
                // Run dsnap_version
                let run = dsnap_version();
                match run {
                    Ok(version) => println!("{}", version),
                    Err(e) => fail!("{}", e),
                }
            }
            // Which snapshot(s) contain a package
            Some(("whichsnap", whichsnap_matches)) => {
                // Get pkgs value
                let pkgs: Vec<String> = whichsnap_matches.get_many::<String>("PACKAGE").unwrap().map(|s| format!("{}", s)).collect();

                // Run which_snapshot_has
                match which_snapshot_has(pkgs) {
                    Ok(_) => (),
                    Err(e) => fail!("{}", e),
                }
            }
            // Which deployment is active
            Some(("whichtmp", _matches)) => {
                // Run print_tmp
                match print_tmp() {
                    Ok(tmp) => println!("{}", tmp),
                    Err(e) => fail!("{}", e),
                }
            }
           _=> unreachable!(), // If all subcommands called, anything else is unreachable
        }
    }
}
            }

    if FAILED.load(Ordering::SeqCst) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
