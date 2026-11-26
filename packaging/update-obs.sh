#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OBS="$ROOT/packaging/obs"

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

mkdir -p "$OBS"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

printf 'Prefer: rust\n' > "$OBS/_config"

# Clean upstream source archive used by the Arch PKGBUILD.
# Exclude packaging/obs so generated OBS artifacts never recurse into the source.
mkdir -p "$TMP/src/distro-snapper-$VERSION"
git -C "$ROOT" archive "$REF" \
  | tar -x -C "$TMP/src/distro-snapper-$VERSION"
rm -rf "$TMP/src/distro-snapper-$VERSION/packaging/obs"
tar -czf "$OBS/distro-snapper-$VERSION.tar.gz" \
  -C "$TMP/src" \
  "distro-snapper-$VERSION"

# Vendored Cargo sources for offline OBS builds.
(
  cd "$ROOT"
  PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 \
    "$CARGO" vendor --locked "$TMP/vendor"
)

# Debian's dh_clean removes *.orig files from the unpacked source tree, but
# Cargo's checksum manifests still reference them. Drop those files and their
# checksum entries before archiving the vendor tree.
find "$TMP/vendor" -type f -name Cargo.toml.orig -delete
python3 - "$TMP/vendor" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
for checksum in root.glob("*/.cargo-checksum.json"):
    data = json.loads(checksum.read_text())
    files = data.get("files", {})
    for name in [
        name
        for name in files
        if pathlib.PurePosixPath(name).name == "Cargo.toml.orig"
    ]:
        del files[name]
    checksum.write_text(json.dumps(data, separators=(",", ":")))
PY

tar -cJf "$OBS/cargo-vendor-$VERSION.tar.xz" -C "$TMP" vendor

# Debian orig archive carries the same vendored sources so it can build offline.
mkdir -p "$TMP/deb"
tar -xzf "$OBS/distro-snapper-$VERSION.tar.gz" -C "$TMP/deb"

DEB_SRC="$TMP/deb/distro-snapper-$VERSION"
mkdir -p "$DEB_SRC/vendor"
cp -a "$TMP/vendor/." "$DEB_SRC/vendor/"

mkdir -p "$DEB_SRC/.cargo"
cat > "$DEB_SRC/.cargo/config.toml" <<'CONFIG'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
CONFIG

tar -czf "$OBS/distro-snapper_$VERSION.orig.tar.gz" \
  -C "$TMP/deb" \
  "distro-snapper-$VERSION"

# Keep exactly one archive of each kind in the OBS package.
find "$OBS" -maxdepth 1 -type f -name 'distro-snapper-*.tar.gz' \
  ! -name "distro-snapper-$VERSION.tar.gz" -delete

find "$OBS" -maxdepth 1 -type f -name 'cargo-vendor-*.tar.xz' \
  ! -name "cargo-vendor-$VERSION.tar.xz" -delete

find "$OBS" -maxdepth 1 -type f -name 'distro-snapper_*.orig.tar.gz' \
  ! -name "distro-snapper_$VERSION.orig.tar.gz" -delete

# The canonical PKGBUILD is OBS-safe and lives at the package root.
if [[ "${SYNC_PKGBUILD:-1}" == "1" ]]; then
  cp "$ROOT/packaging/PKGBUILD" "$OBS/PKGBUILD"
fi

# New release: reset Arch pkgver/pkgrel.
if [[ "${GITHUB_REF_TYPE:-}" == "tag" && -f "$OBS/PKGBUILD" ]]; then
  sed -i "s/^pkgver=.*/pkgver=$VERSION/" "$OBS/PKGBUILD"
  sed -i 's/^pkgrel=.*/pkgrel=1/' "$OBS/PKGBUILD"
fi

# Debian files use OBS debtransform names at the package root.
for f in changelog control copyright rules; do
  cp "$ROOT/packaging/debian/$f" "$OBS/debian.$f"
done

# Preserve the Debian revision on content-only updates; reset on release tags.
REV=1
if [[ "${GITHUB_REF_TYPE:-}" != "tag" && -f "$OBS/distro-snapper.dsc" ]]; then
  current=$(sed -n 's/^Version: *[^ -]*-\([0-9]\+\).*/\1/p' "$OBS/distro-snapper.dsc" | head -n1)
  [[ -n "$current" ]] && REV="$current"
fi

cat > "$OBS/distro-snapper.dsc" <<EOF
Format: 3.0 (quilt)
Source: distro-snapper
Version: $VERSION-$REV
Binary: distro-snapper
Maintainer: Chibani <11yzyv86j@relay.firefox.com>
Architecture: any
Homepage: https://github.com/ochibani/dsnap
Standards-Version: 4.7.0
Build-Depends: debhelper-compat (= 13), cargo, rustc, pkg-config, libssl-dev, libcurl4-openssl-dev, libpython3-dev, file
DEBTRANSFORM-TAR: distro-snapper_$VERSION.orig.tar.gz
EOF
