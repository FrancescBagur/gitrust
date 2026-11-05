#!/usr/bin/env bash
#
# Builds gitrust and puts it where the desktop can find it: the binary on
# the path, the icon in every size the themes ask for, and the entry that
# ties the two together.
#
#   ./packaging/install.sh                 into ~/.local, no root needed
#   PREFIX=/usr/local sudo -E ./packaging/install.sh    for everybody
#
# Nothing is written outside PREFIX. Run uninstall.sh to take it all out.

set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
prefix=${PREFIX:-$HOME/.local}
binary=$here/target/release/gitrust

if [[ ${1:-} == --no-build ]]; then
    [[ -x $binary ]] || { echo "there is no binary at $binary"; exit 1; }
else
    echo "building"
    cargo build --release --manifest-path "$here/Cargo.toml"
fi

install -Dm755 "$binary" "$prefix/bin/gitrust"
install -Dm644 "$here/packaging/gitrust.desktop" \
    "$prefix/share/applications/gitrust.desktop"

for size in 16 32 48 64 128 256; do
    install -Dm644 "$here/assets/icon-$size.png" \
        "$prefix/share/icons/hicolor/${size}x${size}/apps/gitrust.png"
done
install -Dm644 "$here/assets/icon.png" \
    "$prefix/share/icons/hicolor/512x512/apps/gitrust.png"

# Both of these only make the desktop notice sooner. A machine without them
# picks the new entry up on its own, at the next login.
command -v update-desktop-database >/dev/null &&
    update-desktop-database -q "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null &&
    gtk-update-icon-cache -qtf "$prefix/share/icons/hicolor" || true

echo "installed into $prefix"
case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "note: $prefix/bin is not on your PATH" ;;
esac
