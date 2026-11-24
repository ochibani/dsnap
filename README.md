# Distro Snapper

## Translations

[English](README.md) · [العربية](translations/ar/README.md)

**Distro Snapper** is a Rust command-line tool for managing
immutable, tree-shaped Btrfs snapshots on Linux. Each snapshot can keep its own
package profile, configuration and GRUB boot target.

> **Experimental project.** Distro Snapper is under active development.
> Snapshot layouts, command behavior, safety checks and package integration may
> change without compatibility guarantees. Use spare disks or verified backups,
> and read [`docs/partitioning.md`](docs/partitioning.md) before running
> commands that modify disks, snapshots or boot entries.

## Quick links

- [Easy installation path](docs/partitioning.md#easy-installation-path)
- [Hard requirements](docs/partitioning.md#hard-requirements)
- [Recommended layout](docs/partitioning.md#recommended-layout)
- [Build from source](#build-from-source)
- [Build packages](#build-packages)
- [Command reference](docs/commands.md)
- [History and credits](#history-and-credits)

<a id="build-from-source"></a>
## Build from source

Distro Snapper is built with Cargo. The default feature set is
`base-import,btrfs,grub,import,pacman`.

The `apt`, `dnf` and `pacman` features select the package-manager backend at
compile time. They are mutually exclusive: choose exactly one for the target
distribution.

Before building, set the PyO3 compatibility flag:

```sh
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
```

This is needed because the project pins `pyo3 0.21.2` and newer Python
interpreters require forward-compatible stable-ABI mode.

### Cargo features

| Feature | Purpose |
| --- | --- |
| `base-import` | Enables the `base-import` command for restoring snapshot 0 from a verified stream. |
| `btrfs` | Enables Btrfs subvolume and snapshot support; pulls in `libbtrfs`. |
| `grub` | Enables GRUB boot-target and recovery-entry support. |
| `import` | Enables the `import` command. |
| `lock` | Enables package-locking support, such as `apt-mark hold` on Debian-based systems. |
| `apt` | Compile-time APT backend for Debian/Ubuntu builds. |
| `dnf` | Compile-time DNF backend for Fedora builds. |
| `pacman` | Compile-time Pacman backend for Arch builds. |

A plain build uses the default feature set:

```sh
cargo build --locked
```

### Arch Linux

Install the build and runtime dependencies:

```sh
sudo pacman -S --needed base-devel cargo git btrfs-progs curl efibootmgr gnupg python python-anytree grub
```

The Pacman backend can also use `paru` for AUR packages; install it from the AUR
if your profiles need AUR support.

Build with the Arch backend explicitly:

```sh
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build --release --locked --no-default-features \
  --features "base-import,btrfs,grub,import,lock,pacman"
```

### Fedora

Install the build and runtime dependencies:

```sh
sudo dnf install cargo rust gcc make pkgconf-pkg-config openssl-devel \
  libcurl-devel python3-devel btrfs-progs curl efibootmgr gnupg2 \
  python3 python3-anytree
```

Build with the DNF backend explicitly:

```sh
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build --release --locked --no-default-features \
  --features "base-import,btrfs,grub,import,lock,dnf"
```

### Debian / Ubuntu

Install the build and runtime dependencies:

```sh
sudo apt install build-essential cargo rustc pkg-config libssl-dev \
  libcurl4-openssl-dev libpython3-dev file debhelper btrfs-progs curl \
  efibootmgr gnupg python3-anytree
```

Build with the APT backend explicitly:

```sh
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build --release --locked --no-default-features \
  --features "base-import,btrfs,grub,import,lock,apt"
```

<a id="build-packages"></a>
## Build packages

The packaging recipes live under `packaging/`. They install `/usr/bin/dsnap`,
the systemd units, manpages and shell completions. Adding the built package to
a repository is the user's or package manager's responsibility.

### Arch Linux

The PKGBUILD builds the `distro-snapper-git` package:

```sh
cd packaging
makepkg -si
```

### Fedora

Prepare the RPM build tree, source archive and systemd source files:

```sh
sudo dnf install rpmdevtools rpm-build
rpmdev-setuptree

git archive --format=tar.gz --prefix=distro-snapper-0.1.0/ \
  -o ~/rpmbuild/SOURCES/distro-snapper-0.1.0.tar.gz HEAD

mkdir -p ~/rpmbuild/SOURCES/packaging/systemd
cp packaging/systemd/dsnap-auto-upgrade.service \
   packaging/systemd/dsnap-auto-upgrade.timer \
   ~/rpmbuild/SOURCES/packaging/systemd/

rpmbuild -ba packaging/rpm/distro-snapper.spec
```

### Debian / Ubuntu

The Debian packaging files live in `packaging/debian`. Copy them to `debian/`
before building:

```sh
cp -r packaging/debian debian
dpkg-buildpackage -us -uc -b
rm -rf debian
```

The Debian and Fedora recipes use the `apt` and `dnf` features respectively.
The Arch PKGBUILD uses the default Pacman backend.

## History and credits

Distro Snapper did not start from scratch. It is the current name in a line of
projects that developed the immutable, tree-shaped Btrfs snapshot model.

The lineage is:

1. [astOS](https://github.com/lambdanil/astOS), started by
   [@lambdanil](https://github.com/lambdanil), introduced the immutable Arch
   snapshot-tree approach.
2. [AshOS](https://github.com/ashos/ashos) carried that design forward as a
   universal tree-shaped meta-distribution, with major work by
   [@i2](https://github.com/i2), [@lambdanil](https://github.com/lambdanil) and
   other contributors.
3. The [`rust` branch of ochibani/ash](https://github.com/ochibani/ash/tree/rust)
   was the direct technical ancestor of this repository. It explored the Rust
   implementation, package-manager backends and snapshot workflows that later
   became Distro Snapper.
4. [Distro Snapper](https://github.com/ochibani/dsnap), with the `dsnap`
   command, is the current root-only CLI-focused continuation of that work.

Credit is due to the previous developers and contributors who made this
possible:

- **astOS:** [@lambdanil](https://github.com/lambdanil),
  [@MenacingPerson](https://github.com/MenacingPerson),
  [@i2](https://github.com/i2), [@Elecleus](https://github.com/Elecleus),
  [@Rtizie](https://github.com/Rtizie) and
  [@ShalokShalom](https://github.com/ShalokShalom).
- **AshOS / ash:** [@i2](https://github.com/i2),
  [@lambdanil](https://github.com/lambdanil),
  [@vireakvibol](https://github.com/vireakvibol),
  [@duhdugg](https://github.com/duhdugg),
  [@xaqbr](https://github.com/xaqbr),
  [@cybertelx](https://github.com/cybertelx) and
  [@RashiqAzhan](https://github.com/RashiqAzhan).

Thanks also to everyone who reported issues, tested snapshots, wrote
documentation or contributed changes in those projects. Distro Snapper remains
a separate experimental project, but it owes its design and early
implementation work to their efforts.
