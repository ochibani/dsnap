#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OBS="$ROOT/packaging/obs"
ARCH="$OBS/distro-snapper-arch"
DEB="$OBS/distro-snapper-debian"

if [[ "${GITHUB_REF_TYPE:-}" == "tag" ]]; then
  VERSION="${GITHUB_REF_NAME#v}"
  REF="$GITHUB_REF_NAME"
else
  VERSION=$(sed -n 's/^version = "\([^"]*\)".*/\1/p' "$ROOT/Cargo.toml" | head -n1)
  REF="HEAD"
fi

[[ -n "$VERSION" ]] || {
  echo "Could not determine version" >&2
  exit 1
}

CARGO="${CARGO:-cargo}"
if ! command -v "$CARGO" >/dev/null 2>&1 && [[ -x "$HOME/.cargo/bin/cargo" ]]; then
  CARGO="$HOME/.cargo/bin/cargo"
fi

mkdir -p "$ARCH" "$DEB"

printf 'Prefer: rust\n' > "$OBS/_config"

# Source tarball for Arch and Debian.
git -C "$ROOT" archive --format=tar.gz \
  --prefix="distro-snapper-$VERSION/" \
  -o "$ARCH/distro-snapper-$VERSION.tar.gz" \
  "$REF"

cp "$ARCH/distro-snapper-$VERSION.tar.gz" \
  "$DEB/distro-snapper_$VERSION.orig.tar.gz"

# Keep exactly one source archive per OBS package.
find "$ARCH" -maxdepth 1 -type f -name 'distro-snapper-*.tar.gz' \
  ! -name "distro-snapper-$VERSION.tar.gz" -delete

find "$ARCH" -maxdepth 1 -type f -name 'cargo-vendor-*.tar.xz' \
  ! -name "cargo-vendor-$VERSION.tar.xz" -delete

find "$DEB" -maxdepth 1 -type f -name 'distro-snapper_*.orig.tar.gz' \
  ! -name "distro-snapper_$VERSION.orig.tar.gz" -delete

# Vendored Cargo sources for offline OBS builds.
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

(
  cd "$ROOT"
  PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 \
    "$CARGO" vendor --locked "$TMP/vendor"
)

tar -cJf "$ARCH/cargo-vendor-$VERSION.tar.xz" -C "$TMP" vendor

# The canonical PKGBUILD is now OBS-safe.
if [[ "${SYNC_PKGBUILD:-1}" == "1" ]]; then
  cp "$ROOT/packaging/PKGBUILD" "$ARCH/PKGBUILD"
fi

# Debian files use OBS debtransform names.
for f in changelog control copyright rules; do
  cp "$ROOT/packaging/debian/$f" "$DEB/debian.$f"
done

# Preserve the Debian revision on content-only updates; reset on release tags.
REV=1
if [[ "${GITHUB_REF_TYPE:-}" != "tag" && -f "$DEB/distro-snapper.dsc" ]]; then
  current=$(sed -n 's/^Version: *[^ -]*-\([0-9]\+\).*/\1/p' "$DEB/distro-snapper.dsc" | head -n1)
  [[ -n "$current" ]] && REV="$current"
fi

cat > "$DEB/distro-snapper.dsc" <<EOF
Format: 3.0 (quilt)
Source: distro-snapper
Version: $VERSION-$REV
Binary: distro-snapper
Maintainer: Chibani <11yzyv86j@relay.firefox.com>
Architecture: any
Homepage: https://github.com/ochibani/dsnap
Standards-Version: 4.7.0
Build-Depends: debhelper-compat (= 13), cargo, rustc, pkg-config, libssl-dev, libcurl4-openssl-dev, libpython3-dev, file
EOF

# New release: reset Arch pkgver/pkgrel.
if [[ "${GITHUB_REF_TYPE:-}" == "tag" && -f "$ARCH/PKGBUILD" ]]; then
  sed -i "s/^pkgver=.*/pkgver=$VERSION/" "$ARCH/PKGBUILD"
  sed -i 's/^pkgrel=.*/pkgrel=1/' "$ARCH/PKGBUILD"
fi
