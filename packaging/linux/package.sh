#!/usr/bin/env bash
# Package a Linux release build as dist/tonique-<version>-linux-x86_64.tar.gz.
# Usage: packaging/linux/package.sh <version>
set -euo pipefail
version=$1
name=tonique-$version-linux-x86_64
root=dist/$name

rm -rf "$root"
mkdir -p "$root"
install -m755 target/release/tonique_daw "$root/tonique"
install -m755 packaging/linux/install.sh "$root/"
install -m644 packaging/linux/Tonique.desktop "$root/"
install -m644 images/logo.svg "$root/tonique.svg"
install -m644 LICENSE-MIT LICENSE-APACHE "$root/"
tar -C dist -czf "dist/$name.tar.gz" "$name"
echo "dist/$name.tar.gz"
