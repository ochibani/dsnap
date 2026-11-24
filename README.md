# distro snapper

**distro snapper** is a root-only Rust command-line tool for managing
immutable, tree-shaped Btrfs snapshots on Linux. Each snapshot can keep its own
package profile, configuration and GRUB boot target.

The installed command is `dsnap`. Package names use the hyphenated form
`distro-snapper`, with `distro-snapper-git` for Git builds and
`distro-snapper-profiles` for bundled profiles.

> **Experimental project.** distro snapper is under active development.
> Snapshot layouts, command behavior, safety checks and package integration may
> change without compatibility guarantees. Use spare disks or verified backups,
> and read [`docs/partitioning.md`](docs/partitioning.md) before running
> commands that modify disks, snapshots or boot entries.

## Quick links

- [Easy installation path](docs/partitioning.md#easy-installation-path)
- [Hard requirements](docs/partitioning.md#hard-requirements)
- [Recommended layout](docs/partitioning.md#recommended-layout)
- [Command reference](docs/commands.md)
- [History and credits](#history-and-credits)

## History and credits

distro snapper did not start from scratch. It is the current name in a line of
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
   became distro snapper.
4. [distro snapper](https://github.com/ochibani/dsnap), with the `dsnap`
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
  [@ochibani](https://github.com/ochibani),
  [@vireakvibol](https://github.com/vireakvibol),
  [@duhdugg](https://github.com/duhdugg),
  [@xaqbr](https://github.com/xaqbr),
  [@cybertelx](https://github.com/cybertelx) and
  [@RashiqAzhan](https://github.com/RashiqAzhan).

Thanks also to everyone who reported issues, tested snapshots, wrote
documentation or contributed changes in those projects. distro snapper remains
a separate experimental project, but it owes its design and early
implementation work to their efforts.
