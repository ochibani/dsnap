# Command reference for Distro Snapper (`dsnap`)

All examples assume root privileges via `sudo`. The generated manpage remains
the exact source for flags; this page explains intent and gives safe starting
examples.

Badges:

- **read-only** — does not modify snapshots;
- **changes** — modifies a copy or deployment;
- **destructive** — can delete data or change boot state.

## Inspect the system

| Command | What it does | Example |
| --- | --- | --- |
| `current` | Shows the current snapshot number. | `sudo dsnap current` |
| `tree` | Shows the snapshot tree. | `sudo dsnap tree` |
| `list` | Lists packages in a snapshot; `-e` excludes dependencies. | `sudo dsnap list -s 3 -e` |
| `whichsnap` | Finds snapshots containing a package. | `sudo dsnap whichsnap -p firefox` |
| `whichtmp` | Shows the active deployment snapshot. | `sudo dsnap whichtmp` |
| `subvolumes` | Lists Btrfs subvolumes. | `sudo dsnap subvolumes` |
| `last-update` | Checks last update status. | `sudo dsnap last-update` |
| `version` | Prints the dsnap version. | `sudo dsnap version` |
| `difference` | Shows package diff between two snapshots. | `sudo dsnap difference -s 3 -t 4` |
| `description` | Sets or replaces a snapshot description. | `sudo dsnap description -s 3 -d "KDE test"` |

## Create, clone and delete trees

| Command | What it does | Example |
| --- | --- | --- |
| `new-tree` | Creates a top-level tree from snapshot 0. | `sudo dsnap new-tree -d "GNOME workstation"` |
| `add-branch` | Creates a child snapshot. | `sudo dsnap add-branch -s 3 -d "try NVIDIA driver"` |
| `clone` | Creates a top-level tree from a snapshot copy. | `sudo dsnap clone -s 3 -d "backup copy"` |
| `clone-branch` | Creates a sibling snapshot. | `sudo dsnap clone-branch -s 3` |
| `clone-tree` | Recursively clones a snapshot and descendants. | `sudo dsnap clone-tree -s 3` |
| `clone-under` | Copies a snapshot under another parent. | `sudo dsnap clone-under -s 2 -b 7` |
| `rebuild` | Rebuilds a fresh tree from a saved profile. | `sudo dsnap rebuild -s 3 -d "fresh rebuild"` |
| `delete` | Deletes a snapshot and descendants. | `sudo dsnap delete -s 7` |
| `reset` | Deletes all non-base snapshots, deploys snapshot 0 and reboots. | `sudo dsnap reset` |

## Packages, profiles and services

| Command | What it does | Example |
| --- | --- | --- |
| `install` | Installs packages or profiles. | `sudo dsnap install -s 3 -p vim git` |
| `install` profile | Installs a bundled profile. | `sudo dsnap install -s 3 -P desktop-gnome` |
| `uninstall` | Removes packages or profiles. | `sudo dsnap uninstall -s 3 -p vim` |
| `upgrade` | Upgrades packages in a snapshot. | `sudo dsnap upgrade -s 3 -y` |
| `auto-upgrade` | Noninteractive upgrade path. | `sudo dsnap auto-upgrade -s 3` |
| `refresh` | Refreshes repository metadata. | `sudo dsnap refresh -s 3` |
| `fixdb` | Rebuilds keyring/package database. | `sudo dsnap fixdb -s 3` |
| `edit-profile` | Edits a snapshot profile. | `sudo dsnap edit-profile -s 3` |
| `edit-config` | Edits `dsnap.conf`. | `sudo dsnap edit-config -s 3` |
| `service` | Enables or disables systemd units. | `sudo dsnap service -s 3 -e sshd.service` |
| `tree-install` | Installs throughout a tree. | `sudo dsnap tree-install -t 3 -P desktop-gnome` |
| `tree-uninstall` | Uninstalls throughout a tree. | `sudo dsnap tree-uninstall -t 3 -p vim` |
| `tree-upgrade` | Upgrades a tree. | `sudo dsnap tree-upgrade -t 3` |
| `tree-sync` | Upgrades and syncs a tree recursively. | `sudo dsnap tree-sync -t 3` |

## Deploy, recovery and chroots

| Command | What it does | Example |
| --- | --- | --- |
| `deploy` | Sets the next-boot default. | `sudo dsnap deploy -s 3` |
| `boot-update` | Regenerates GRUB configuration. | `sudo dsnap boot-update -s 3` |
| `efi-update` | Switches EFI boot entry interactively. | `sudo dsnap efi-update` |
| `etc-update` | Saves live `/etc` changes. | `sudo dsnap etc-update` |
| `rollback` | Deploys a clone of the previous deployment. | `sudo dsnap rollback` |
| `hollow` | Mounts a snapshot writable at `/`. | `sudo dsnap hollow -s 3` |
| `chroot` | Opens a confined shell in a writable copy. | `sudo dsnap chroot -s 3` |
| `live-chroot` | Recovery shell in the booted snapshot. | `sudo dsnap live-chroot` |
| `run` | Runs a command in a writable copy. | `sudo dsnap run -s 3 -c "vim /etc/fstab"` |
| `tree-run` | Runs a command throughout a tree. | `sudo dsnap tree-run -t 3 -c "update-ca-certificates"` |
| `unlock` | Discards uncommitted chroot changes. | `sudo dsnap unlock -s 3` |
| `clean-tmp` | Cleans unmounted chroot working snapshots. | `sudo dsnap clean-tmp` |
| `immutable-enable` | Enables snapshot immutability. | `sudo dsnap immutable-enable -s 3` |
| `immutable-disable` | Disables snapshot immutability. | `sudo dsnap immutable-disable -s 3` |
| `base-update` | Updates snapshot 0 and recovery deployment. | `sudo dsnap base-update -y` |
| `base-rebuild` | Rebuilds snapshot 0 from bootstrap. | `sudo dsnap base-rebuild` |

## Import and export

| Command | What it does | Example |
| --- | --- | --- |
| `export` | Exports a signed compressed Btrfs stream. | `sudo dsnap export -s 3 -o /srv/dsnap-exports -k ABC123` |
| `import` | Imports a verified stream. | `sudo dsnap import -p /srv/dsnap-exports/stream -d "restored tree"` |
| `base-import` | Replaces snapshot 0 from a verified stream. | `sudo dsnap base-import -p /srv/dsnap-exports/base-stream` |

Import paths are illustrative. Keep the `.sha512` and `.sig` sidecars beside
the stream and avoid `--allow-unverified` except for trusted, reviewed data.
