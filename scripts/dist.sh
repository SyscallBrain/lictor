#!/usr/bin/env bash
# Builds the release files in dist/:
#   lictor-VERSION-x86_64-unknown-linux-musl.tar.gz  static binary for any Linux
#   lictor_VERSION-1_amd64.deb                       Debian/Ubuntu package
#   lictor-VERSION-1-x86_64.pkg.tar.zst              Arch Linux package
#   SHA256SUMS
# Requires: rustup target x86_64-unknown-linux-musl, cargo-deb, and makepkg or
# Docker for the Arch package.
set -euo pipefail
cd "$(dirname "$0")/.."

target=x86_64-unknown-linux-musl
version=$(cargo metadata --no-deps --format-version 1 |
  python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')

cargo build --release --locked --target "$target"
binary="target/$target/release/lictor"

# Man page and shell completions come from the same definitions as --help.
assets=target/dist-assets
rm -rf "$assets" && mkdir -p "$assets"
"$binary" --generate man | gzip -9n > "$assets/lictor.1.gz"
"$binary" --generate bash > "$assets/lictor.bash"
"$binary" --generate zsh > "$assets/_lictor"
"$binary" --generate fish > "$assets/lictor.fish"

rm -rf dist && mkdir -p dist
package="lictor-$version-$target"
mkdir -p "dist/$package/completions"
cp "$binary" LICENSE README.md "dist/$package/"
cp "$assets/lictor.1.gz" "dist/$package/"
cp "$assets/lictor.bash" "$assets/_lictor" "$assets/lictor.fish" "dist/$package/completions/"
tar -C dist -czf "dist/$package.tar.gz" --owner=0 --group=0 --mode=go-w "$package"
rm -rf "dist/${package:?}"

cargo deb --no-build --target "$target" --output dist/
scripts/arch-package.sh "$version"

cd dist
sha256sum -- *.tar.gz *.deb *.pkg.tar.zst > SHA256SUMS
cat SHA256SUMS
