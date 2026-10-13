#!/usr/bin/env bash
# Builds dist/lictor-VERSION-1-x86_64.pkg.tar.zst from the static archive that
# scripts/dist.sh made, with makepkg. Runs makepkg directly on Arch Linux and in
# an archlinux:base-devel container elsewhere.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$1
tarball="dist/lictor-$version-x86_64-unknown-linux-musl.tar.gz"
build=$(mktemp -d)
trap 'rm -rf "$build"' EXIT

checksum=$(sha256sum "$tarball" | cut -d' ' -f1)
sed -e "s/@VERSION@/$version/" -e "s/@SHA256@/$checksum/" packaging/arch/PKGBUILD > "$build/PKGBUILD"
cp "$tarball" "$build/"

if command -v makepkg >/dev/null; then
  (cd "$build" && makepkg --clean --nodeps --force)
else
  # makepkg refuses to run as root; build as an unprivileged user, then give
  # the files back to the calling user so the cleanup trap can remove them.
  docker run --rm -v "$build:/build" -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    archlinux:base-devel bash -euc '
      useradd --create-home builder
      chown -R builder /build
      su builder -c "cd /build && makepkg --clean --nodeps --force"
      chown -R "$HOST_UID:$HOST_GID" /build'
fi
cp "$build"/lictor-"$version"-*-x86_64.pkg.tar.zst dist/
