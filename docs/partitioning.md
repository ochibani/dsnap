# Partitioning for Distro Snapper (`dsnap`)

This guide describes the Btrfs layout expected by **Distro Snapper**.

<a id="easy-installation-path"></a>
## Easy installation path

The recommended way to get the correct Btrfs layout is to use the companion
tools instead of partitioning manually:

- use [`dinstaller`](https://github.com/ochibani/dinstaller) to install Distro Snapper with the expected subvolume layout;
- use the [`dlive`](https://github.com/ochibani/dlive) script to build a live
  ISO that contains [`dinstaller`](https://github.com/ochibani/dinstaller) and Distro Snapper.

Manual partitioning is still supported, but the easiest path is:

```text
build live ISO with dlive → boot the ISO → install with dinstaller
```

This avoids common layout mistakes such as putting `/.snapshots` inside the
live root subvolume or using an unsupported `/boot` layout.

<a id="install-with-dinstaller"></a>
### Install with dinstaller

[`dinstaller`](https://github.com/ochibani/dinstaller) is the recommended installer path for Distro Snapper. With a
dsnap-aware configuration, it should create or verify:

```text
@_linux
@boot_linux
@etc_linux
@var_linux
@.snapshots_linux
```

It should also configure:

```text
/.snapshots
/.snapshots/dsnap/part
/boot
/boot/efi
```

<a id="build-a-live-iso-with-dlive"></a>
### Build a live ISO with dlive

The [`dlive`](https://github.com/ochibani/dlive) archiso profile can build a live ISO
that includes [`dinstaller`](https://github.com/ochibani/dinstaller) and Distro Snapper.

```sh
pacman -S archiso
mkarchiso -v -r -w /path/to/work_dir -o /path/to/out_dir /path/to/dlive/
example: mkarchiso -v -r -w /tmp/dinstalleriso-tmp -o ~/out_dir ~/dlive
```

<a id="scope"></a>
## Scope

This guide covers the layout used by Distro Snapper for Btrfs-root systems with
GRUB. It does not replace distribution-specific installation documentation.

<a id="hard-requirements"></a>
## Hard requirements

- Btrfs root filesystem.
- GRUB with EFI or BIOS boot support.
- Root subvolume `@_linux`.
- Boot subvolume `@boot_linux` mounted at `/boot`.
- `@etc_linux` and `@var_linux` subvolumes.
- `@.snapshots_linux` mounted at `/.snapshots`, outside `@_linux`.
- `/.snapshots/dsnap/part` exists and is writable by root.
- `@srv_linux` as a separate shared subvolume at `/srv`, when used.

<a id="recommended-layout"></a>
## Recommended layout

```text
@_linux                 root
@boot_linux             /boot
@etc_linux              /etc
@var_linux              /var
@.snapshots_linux       /.snapshots
@srv_linux              /srv
```

Shared `/home`, `/srv` and default `/var` behavior is intentional.

<a id="example-initial-fstab"></a>
## Example initial fstab

```fstab
UUID=ROOT / btrfs subvol=@_linux,compress=zstd:1 0 0
UUID=ROOT /boot btrfs subvol=@boot_linux,compress=zstd:1 0 0
UUID=ROOT /etc btrfs subvol=@etc_linux,compress=zstd:1 0 0
UUID=ROOT /var btrfs subvol=@var_linux,compress=zstd:1 0 0
UUID=ROOT /.snapshots btrfs subvol=@.snapshots_linux,compress=zstd:1 0 0
UUID=ROOT /srv btrfs subvol=@srv_linux,compress=zstd:1 0 0
UUID=EFI /boot/efi vfat umask=0077 0 2
```

<a id="shared-mutable-directories"></a>
## Shared mutable directories

`/home`, `/srv` and the default `/var` data are shared between trees.

<a id="common-mistakes"></a>
## Common mistakes

- `/.snapshots` inside `@_linux`.
- Missing `@boot_linux` or unsupported `/boot` layout.
- Putting GRUB outputs inside a snapshot that will be rolled back.
- Treating shared `/home`, `/srv` or `/var` as immutable.
- Running destructive tests on production disks.
