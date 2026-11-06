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

# In a clone the binary is where cargo leaves it. In the tarball from a
# release there is no target/ at all and the binary sits at the top, beside
# packaging/ and assets/. Both are taken, so that the one command printed in
# the release notes is the command that works.
if [[ ! -x $binary && -x $here/gitrust ]]; then
    binary=$here/gitrust
fi

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

# The icon is on disk and correct, and the dock may still show the old one or
# none at all. Panels read the icon theme once and keep what they found for
# as long as they are running -- which, on a machine nobody reboots, is days.
# The files above change nothing about that.
#
# Worth saying out loud because the alternative is somebody deciding the
# package is broken. It is the last thing printed, so it is the thing still
# on screen when they look at the dock and see nothing.
cat <<'NOTE'

If the icon does not appear, or the old one is still there, the desktop is
showing you what it remembered. Restart its panel, or log out and back in:

    GNOME on X11    Alt+F2, then r
    COSMIC          pkill -x cosmic-panel
    KDE             plasmashell --replace &
    anything else   log out and back in

Nothing is lost either way: a panel keeps no state worth keeping.
NOTE
