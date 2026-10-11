#!/bin/sh
# Install Tonique from this folder for the current user, or for everyone
# with `sudo PREFIX=/usr/local ./install.sh`. `--uninstall` removes it.
set -eu
cd "$(dirname "$0")"
prefix=${PREFIX:-$HOME/.local}
bin=$prefix/bin/tonique
icon=$prefix/share/icons/hicolor/scalable/apps/tonique.svg
desktop=$prefix/share/applications/tonique.desktop

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin" "$icon" "$desktop"
    echo "Tonique uninstalled from $prefix"
    exit 0
fi

install -Dm755 tonique "$bin"
install -Dm644 tonique.svg "$icon"
mkdir -p "$(dirname "$desktop")"
# Absolute path: $prefix/bin may not be in the desktop's PATH.
sed "s|^Exec=.*|Exec=$bin|" Tonique.desktop > "$desktop"
echo "Tonique installed in $prefix"
