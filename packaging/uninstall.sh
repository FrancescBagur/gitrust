#!/usr/bin/env bash
#
# Takes out exactly what install.sh put in, and nothing else: it removes
# files by name rather than by pattern, so a folder it shares with other
# programs comes out of this untouched.
#
#   ./packaging/uninstall.sh
#   PREFIX=/usr/local sudo -E ./packaging/uninstall.sh

set -euo pipefail

prefix=${PREFIX:-$HOME/.local}

rm -f "$prefix/bin/gitrust"
rm -f "$prefix/share/applications/gitrust.desktop"
for size in 16 32 48 64 128 256 512; do
    rm -f "$prefix/share/icons/hicolor/${size}x${size}/apps/gitrust.png"
done

command -v update-desktop-database >/dev/null &&
    update-desktop-database -q "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null &&
    gtk-update-icon-cache -qtf "$prefix/share/icons/hicolor" || true

echo "removed from $prefix"
echo "nothing in any repository was touched"
