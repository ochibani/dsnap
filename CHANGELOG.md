# Changelog

All notable changes to this project are documented in this file.

This file is generated from conventional commit subjects.
## Changes


### Features


- Generate manpages and fix misleading comments


### Fixes


- Sync_time clock sync, cp exit status checks, rename systemd units

- Remove invalid --confirm flags, fix sh -c pkg arg in dnf/apt uninstall

- Commit snapshot config edits when EDITOR is set

- Rewrite recovery entry removal without String::retain

- Skip profile reconciliation for live tree sync target

- Never wipe the shared mutable_dirs store

- Tear down all hollow mounts before deleting the chroot

- Use apt-mark hold instead of ineffective pin preferences in lockpkg

- Report errors instead of panicking in five dispatcher branches

- Correct inverted help text for sync --live

- Make distro fallback detection deterministic and sane

- Check the cp exit status when copying the GRUB image

- Check cp exit status in the package-manager backends

- Handle a broken or empty EDITOR in snapshot_config_edit

- Recurse_tree emits each node once instead of duplicating intermediates

- Import_tree_file reports an empty tree file instead of panicking

- Tremove accepts --user-profile without --package

- Clone_under copies the branch's content, clone_recursive tracks clones correctly

- Check_profile no longer re-adds removed profile-packages entries

- Uninstall_profile* validate uninstall-commands section, not install-commands

- Upgrade() reports post_transactions failures instead of swallowing them

- Pkg_query uses the right package manager and returns the version

- Switch_to_windows passes efibootmgr arguments correctly

- Export() quotes the destination and checks the pipeline status

- Import/import_base quote the receive path and check btrfs receive

- Install_profile_live targets the live snapshot for services

- Live flows release their mounts on every early return

- Chroot cleans up and reports when check_profile fails

- Install_profile* --force load the profile from the snapshot, not the host

- Hollow() makes the snapshot mutable before deploying it

- Deploy() strips ,ro from the root fstab entry, not the wrong slot name

- Dsnap_umounts removes only the stub file, not /run/systemd/resolve

- Detect the recovery snapshot subvolume with try_exists, not is_file

- Avoid out-of-bounds quote indexing in grub recovery title rewrite

- Lockpkg rewrites the HoldPkg line without corrupting pacman.conf

- Fix_package_db uses chroot paths in chroot mode and handles missing SUDO_USER

- Install_package_helper_live reports missing AUR for unofficial packages

- Correct dnf/apt cache_copy directions and dnf refresh helper

- Do not pass an empty package spec in dnf/apt uninstall helpers

- Harden tmp/partition detection and refresh error reporting (L4-L6)

- Simplify identical profile-empty checks in install_profile* (L7)

- Tighten grub cleanup/recovery handling and package helper robustness (L8,L9,L11-L13,L15)

- Make grub directory discovery deterministic and fallible (L10)

- Run live profile install/uninstall commands in the live snapshot

- Harden snapshot profile/config editing and commit changes reliably

- Refuse destructive chroot cleanup while mounts remain (H4)

- Harden hollow teardown on commit and discard paths (H4)

- Propagate uninstall and live install/uninstall failures

- Propagate errors in chroot, live profiles, and listing

- Avoid panics in mount checks and mutable dir unmounts

- Harden snapshot profile install/uninstall handling

- Handle current snapshot lookup failures

- Handle mount state read failures in print_tmp

- Avoid panics checking rebuild snapshot locks

- Harden snapshot import temp handling

- Avoid panics checking package cache directories

- Harden cli import export and list error handling

- Avoid panics when allocating snapshot numbers

- Propagate mount state read failures

- Propagate deployed snapshot lookup failures

- Propagate tree operation failures

- Propagate immutability toggle failures

- Avoid panics loading profile configuration

- Handle deployment state write failures

- Handle fixdb cleanup failures

- Handle reboot command failures

- Avoid panics reading profile sections

- Handle pacman auto-upgrade command failures

- Propagate package list query failures

- Propagate tree print failures

- Avoid remaining unwrap panics

- Avoid panics parsing distribution release files

- Avoid path conversion panics in pacman backend

- Propagate upgrade command failures

- Avoid path conversion panics in grub backend

- Avoid panics parsing distro metadata

- Propagate signal handler setup failures

- Make AUR config validation fallible

- Keep tree listing tolerant of missing snapshot marker

- Remove unused csv dependency and DirEntry import

- Rename config edit command and add profile alias

- Add typed parser for snapshot configuration

- Preserve profile comments across INI rewrites

- Rework systemd service control in profiles

- Add live upgrade for deployed snapshot

- Alphabetize import lists and disjoint match arms

- Unmount GRUB boot mount on all error paths

- Write GRUB and fstab files atomically

- Parse GRUB menuentries with brace counting

- Build GRUB update commands without a shell

- Clean up chroot when boot update fails

- Replace deployment paths with exact token boundaries

- Use Recovery Mode title for GRUB recovery entry

- Restore Recovery Snapshot GRUB entry title

- Use RECOVERY_TITLE for GRUB recovery entries

- Restore dnf package list queries

- Restore apt package list queries

- Install dsnap instead of paru during bootstrap

- Restore paru bootstrap target

- Rename packages to distro-snapper

- Rename dsnap-profiles to distro-snapper-profiles

- Complete distro-snapper rename

- Clarify CLI definition comment

- Clarify CLI and package descriptions

- Remove hide command

- Rename check command to last-update

- Clarify tmp command

- Clarify install command

- Clarify uninstall command

- Clarify service command

- Clarify auto-upgrade help

- Clarify base-import help and path argument

- Clarify base-rebuild help

- Clarify base-update help

- Clarify boot help

- Clarify branch help

- Clarify chroot help

- Clarify clone help

- Clarify clone-branch help

- Clarify clone-tree help

- Clarify clone-under help and arguments

- Clarify del help and flags

- Clarify deploy help

- Capitalize desc help

- Clarify edit-profile help

- Clarify efi-update help

- Clarify etc-update help

- Clarify export help

- Clarify fixdb help

- Clarify hollow help

- Rename immdis to immutable-disable

- Rename immen to immutable-enable

- Clarify import help and remove unused snapshot option

- Clarify install flags

- Clarify list help

- Clarify new help

- Clarify rebuild help

- Clarify refresh help

- Clarify reset help

- Clarify rollback help

- Clarify run help and command argument

- Clarify sub help

- Clarify sync help and arguments

- Clarify tinstall help and arguments

- Rename tmp to clean-tmp

- Rename tremove to tree-uninstall

- Rename trun to tree-run

- Rename tupgrade to tree-upgrade

- Clarify uninstall flags

- Clarify unlock help

- Clarify upgrade help and flags

- Clarify whichsnap help and package argument

- Order CLI commands alphabetically

- Rename boot to boot-update and adjust aliases

- Rename branch to add-branch and adjust aliases

- Set clean-tmp aliases to tmp and cleantmp

- Rename del to delete and adjust aliases

- Rename desc to description and keep desc alias

- Rename diff to difference and keep diff aliases

- Set immutable-disable aliases to immdis and dim

- Set immutable-enable aliases to disable-mutability and dmut

- Replace immdis alias with mut for immutable-disable

- Rename new to new-tree and keep new alias

- Rename sub to subvolumes and adjust aliases

- Rename sync to tree-sync and adjust aliases

- Rename tinstall to tree-install and adjust aliases

- Simplify uninstall aliases

- Change uninstall alias un to uin

- Dispatch subcommands by canonical names only

- Reject unsafe mutable directory paths

- Bind shared srv directory into snapshot chroots

- Mount private devpts and shm in snapshot chroots

- Build Debian and RPM packages with native package managers

- Return non-zero status on CLI errors

- Align dsnap binary paths with installed /usr/bin location

- Use non-native Debian source format

- Fail when pacman keyring refresh fails

- Deploy the rollback clone returned by clone_as_tree

- Avoid process-wide DEBIAN_FRONTEND changes

- Replace vague optional value comments

- Feature-gate btrfs module in binary

- Check reboot result in reset

- Propagate interactive chroot exit status

- Report base import post-transaction failures

- Report fixdb post-transaction failures

- Avoid service success message for no-op

- Print last-update failure status on stdout

- Rewrite placeholder fstab paths on deploy

- Rewrite etc subvolume fstab on deploy

- Delete nested subvolumes when removing snapshots

- Report clone-under destination accurately

- Fail export when btrfs send fails

- Avoid export filename collisions

- Stop yes-no prompt on closed input

- Import compressed export streams

- Populate imported chroot boot etc and var

- Tolerate already-detached mounts in hollow teardown

- Isolate hollow bind mounts before teardown

- Avoid duplicate mutable marker in descriptions

- Pass DEBIAN_FRONTEND via env in apt upgrade chroot

- Preserve apt archives dir when cleaning base cache

- Install debian package files into distro-snapper dir

- Harden privileged command execution and state integrity

- Label live-chroot as an administrative recovery action

- Verify signed SHA-512 snapshot imports

- Sandbox chroot shells and commands

- Require verified time before trusting clock

- Require trusted user profiles

- Pin import stream before verification

- Harden export destination writes

- Sanitize helper environment and editor path

## [unreleased]

### 🚀 Features

- Generate manpages and fix misleading comments

### 🐛 Bug Fixes

- Sync_time clock sync, cp exit status checks, rename systemd units
- Remove invalid --confirm flags, fix sh -c pkg arg in dnf/apt uninstall
- Commit snapshot config edits when EDITOR is set
- Rewrite recovery entry removal without String::retain
- Skip profile reconciliation for live tree sync target
- Never wipe the shared mutable_dirs store
- Tear down all hollow mounts before deleting the chroot
- Use apt-mark hold instead of ineffective pin preferences in lockpkg
- Report errors instead of panicking in five dispatcher branches
- Correct inverted help text for sync --live
- Make distro fallback detection deterministic and sane
- Check the cp exit status when copying the GRUB image
- Check cp exit status in the package-manager backends
- Handle a broken or empty EDITOR in snapshot_config_edit
- Recurse_tree emits each node once instead of duplicating intermediates
- Import_tree_file reports an empty tree file instead of panicking
- Tremove accepts --user-profile without --package
- Clone_under copies the branch's content, clone_recursive tracks clones correctly
- Check_profile no longer re-adds removed profile-packages entries
- Uninstall_profile* validate uninstall-commands section, not install-commands
- Upgrade() reports post_transactions failures instead of swallowing them
- Pkg_query uses the right package manager and returns the version
- Switch_to_windows passes efibootmgr arguments correctly
- Export() quotes the destination and checks the pipeline status
- Import/import_base quote the receive path and check btrfs receive
- Install_profile_live targets the live snapshot for services
- Live flows release their mounts on every early return
- Chroot cleans up and reports when check_profile fails
- Install_profile* --force load the profile from the snapshot, not the host
- Hollow() makes the snapshot mutable before deploying it
- Deploy() strips ,ro from the root fstab entry, not the wrong slot name
- Dsnap_umounts removes only the stub file, not /run/systemd/resolve
- Detect the recovery snapshot subvolume with try_exists, not is_file
- Avoid out-of-bounds quote indexing in grub recovery title rewrite
- Lockpkg rewrites the HoldPkg line without corrupting pacman.conf
- Fix_package_db uses chroot paths in chroot mode and handles missing SUDO_USER
- Install_package_helper_live reports missing AUR for unofficial packages
- Correct dnf/apt cache_copy directions and dnf refresh helper
- Do not pass an empty package spec in dnf/apt uninstall helpers
- Harden tmp/partition detection and refresh error reporting (L4-L6)
- Simplify identical profile-empty checks in install_profile* (L7)
- Tighten grub cleanup/recovery handling and package helper robustness (L8,L9,L11-L13,L15)
- Make grub directory discovery deterministic and fallible (L10)
- Run live profile install/uninstall commands in the live snapshot
- Harden snapshot profile/config editing and commit changes reliably
- Refuse destructive chroot cleanup while mounts remain (H4)
- Harden hollow teardown on commit and discard paths (H4)
- Propagate uninstall and live install/uninstall failures
- Propagate errors in chroot, live profiles, and listing
- Avoid panics in mount checks and mutable dir unmounts
- Harden snapshot profile install/uninstall handling
- Handle current snapshot lookup failures
- Handle mount state read failures in print_tmp
- Avoid panics checking rebuild snapshot locks
- Harden snapshot import temp handling
- Avoid panics checking package cache directories
- Harden cli import export and list error handling
- Avoid panics when allocating snapshot numbers
- Propagate mount state read failures
- Propagate deployed snapshot lookup failures
- Propagate tree operation failures
- Propagate immutability toggle failures
- Avoid panics loading profile configuration
- Handle deployment state write failures
- Handle fixdb cleanup failures
- Handle reboot command failures
- Avoid panics reading profile sections
- Handle pacman auto-upgrade command failures
- Propagate package list query failures
- Propagate tree print failures
- Avoid remaining unwrap panics
- Avoid panics parsing distribution release files
- Avoid path conversion panics in pacman backend
- Propagate upgrade command failures
- Avoid path conversion panics in grub backend
- Avoid panics parsing distro metadata
- Propagate signal handler setup failures
- Make AUR config validation fallible
- Keep tree listing tolerant of missing snapshot marker
- Remove unused csv dependency and DirEntry import
- Rename config edit command and add profile alias
- Add typed parser for snapshot configuration
- Preserve profile comments across INI rewrites
- Rework systemd service control in profiles
- Add live upgrade for deployed snapshot
- Alphabetize import lists and disjoint match arms
- Unmount GRUB boot mount on all error paths
- Write GRUB and fstab files atomically
- Parse GRUB menuentries with brace counting
- Build GRUB update commands without a shell
- Clean up chroot when boot update fails
- Replace deployment paths with exact token boundaries
- Use Recovery Mode title for GRUB recovery entry
- Restore Recovery Snapshot GRUB entry title
- Use RECOVERY_TITLE for GRUB recovery entries
- Restore dnf package list queries
- Restore apt package list queries
- Install dsnap instead of paru during bootstrap
- Restore paru bootstrap target
- Rename packages to distro-snapper
- Rename dsnap-profiles to distro-snapper-profiles
- Complete distro-snapper rename
- Clarify CLI definition comment
- Clarify CLI and package descriptions
- Remove hide command
- Rename check command to last-update
- Clarify tmp command
- Clarify install command
- Clarify uninstall command
- Clarify service command
- Clarify auto-upgrade help
- Clarify base-import help and path argument
- Clarify base-rebuild help
- Clarify base-update help
- Clarify boot help
- Clarify branch help
- Clarify chroot help
- Clarify clone help
- Clarify clone-branch help
- Clarify clone-tree help
- Clarify clone-under help and arguments
- Clarify del help and flags
- Clarify deploy help
- Capitalize desc help
- Clarify edit-profile help
- Clarify efi-update help
- Clarify etc-update help
- Clarify export help
- Clarify fixdb help
- Clarify hollow help
- Rename immdis to immutable-disable
- Rename immen to immutable-enable
- Clarify import help and remove unused snapshot option
- Clarify install flags
- Clarify list help
- Clarify new help
- Clarify rebuild help
- Clarify refresh help
- Clarify reset help
- Clarify rollback help
- Clarify run help and command argument
- Clarify sub help
- Clarify sync help and arguments
- Clarify tinstall help and arguments
- Rename tmp to clean-tmp
- Rename tremove to tree-uninstall
- Rename trun to tree-run
- Rename tupgrade to tree-upgrade
- Clarify uninstall flags
- Clarify unlock help
- Clarify upgrade help and flags
- Clarify whichsnap help and package argument
- Order CLI commands alphabetically
- Rename boot to boot-update and adjust aliases
- Rename branch to add-branch and adjust aliases
- Set clean-tmp aliases to tmp and cleantmp
- Rename del to delete and adjust aliases
- Rename desc to description and keep desc alias
- Rename diff to difference and keep diff aliases
- Set immutable-disable aliases to immdis and dim
- Set immutable-enable aliases to disable-mutability and dmut
- Replace immdis alias with mut for immutable-disable
- Rename new to new-tree and keep new alias
- Rename sub to subvolumes and adjust aliases
- Rename sync to tree-sync and adjust aliases
- Rename tinstall to tree-install and adjust aliases
- Simplify uninstall aliases
- Change uninstall alias un to uin
- Dispatch subcommands by canonical names only
- Reject unsafe mutable directory paths
- Bind shared srv directory into snapshot chroots
- Mount private devpts and shm in snapshot chroots
- Build Debian and RPM packages with native package managers
- Return non-zero status on CLI errors
- Align dsnap binary paths with installed /usr/bin location
- Use non-native Debian source format
- Fail when pacman keyring refresh fails
- Deploy the rollback clone returned by clone_as_tree
- Avoid process-wide DEBIAN_FRONTEND changes
- Replace vague optional value comments
- Feature-gate btrfs module in binary
- Check reboot result in reset
- Propagate interactive chroot exit status
- Report base import post-transaction failures
- Report fixdb post-transaction failures
- Avoid service success message for no-op
- Print last-update failure status on stdout
- Rewrite placeholder fstab paths on deploy
- Rewrite etc subvolume fstab on deploy
- Delete nested subvolumes when removing snapshots
- Report clone-under destination accurately
- Fail export when btrfs send fails
- Avoid export filename collisions
- Stop yes-no prompt on closed input
- Import compressed export streams
- Populate imported chroot boot etc and var
- Tolerate already-detached mounts in hollow teardown
- Isolate hollow bind mounts before teardown
- Avoid duplicate mutable marker in descriptions
- Pass DEBIAN_FRONTEND via env in apt upgrade chroot
- Preserve apt archives dir when cleaning base cache
- Install debian package files into distro-snapper dir
- Harden privileged command execution and state integrity
- Label live-chroot as an administrative recovery action
- Verify signed SHA-512 snapshot imports
- Sandbox chroot shells and commands
- Require verified time before trusting clock
- Require trusted user profiles
- Pin import stream before verification
- Harden export destination writes
- Sanitize helper environment and editor path

### 💼 Other

- Use distro anytree packages and drop sed
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial dsnap project (renamed from ash): tree-shaped snapshot management for immutable distributions.
