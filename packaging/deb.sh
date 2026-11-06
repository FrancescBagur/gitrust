#!/usr/bin/env bash
#
# Builds the .deb: the binary, the entry, the icons, the licence.
#
#   ./packaging/deb.sh              builds first, then packs
#   ./packaging/deb.sh --no-build   packs what is already in target/release
#
# The package lands in target/ and is named the way Debian names things,
# which is not how we name anything else: gitrust_0.1.0_amd64.deb.
#
# A .deb is a tree of files and one control file, and that is all this makes.
# Doing it by hand rather than with a packaging crate keeps the dependency
# list of this project at what it is, and there is nothing here worth a
# toolchain: the hard part is the one paragraph below, and no tool does it.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
binary=$here/target/release/gitrust

if [[ ${1:-} == --no-build ]]; then
    [[ -x $binary ]] || { echo "there is no binary at $binary"; exit 1; }
else
    cargo build --release --manifest-path "$here/Cargo.toml"
fi

version=$(grep -m1 '^version' "$here/Cargo.toml" | cut -d'"' -f2)
arch=$(dpkg-architecture -qDEB_HOST_ARCH)
root=$here/target/deb
out=$here/target/gitrust_${version}_${arch}.deb

rm -rf "$root"
install -Dm755 "$binary" "$root/usr/bin/gitrust"
install -Dm644 "$here/packaging/gitrust.desktop" \
    "$root/usr/share/applications/gitrust.desktop"
install -Dm644 "$here/LICENSE" "$root/usr/share/doc/gitrust/copyright"
install -Dm644 "$here/README.md" "$root/usr/share/doc/gitrust/README.md"
for size in 16 32 48 64 128 256; do
    install -Dm644 "$here/assets/icon-$size.png" \
        "$root/usr/share/icons/hicolor/${size}x${size}/apps/gitrust.png"
done
install -Dm644 "$here/assets/icon.png" \
    "$root/usr/share/icons/hicolor/512x512/apps/gitrust.png"

# What it needs, in two halves, because one tool can only see one of them.
#
# dpkg-shlibdeps reads what the binary is linked against, and that is a short
# list: the C library, OpenSSL and zlib. Everything a window is made of is
# missing from it, because winit and glutin do not link X11, Wayland, GL or
# dbus -- they open them by name while running, so that one binary works on a
# machine with either display server. Nothing in the file says they are
# wanted, and a package built from that reading installs cleanly and then
# fails to start.
#
# So the ones opened by name are listed here by hand. They were read out of
# the binary with `strings` and matched to their packages with `dpkg -S`;
# `strings target/release/gitrust | grep -oE 'lib[A-Za-z0-9_-]+\.so[0-9.]*'`
# is how to check this list still says the truth after a dependency moves.
opened_at_runtime="libdbus-1-3, libgl1, libwayland-client0, libwayland-egl1, \
libx11-6, libx11-xcb1, libxcursor1, libxi6, libxkbcommon0, libxkbcommon-x11-0, \
libxrandr2"

mkdir -p "$root/DEBIAN"

# dpkg-shlibdeps will not say a word without a debian/control beside it --
# it is meant to run inside a source package and this is not one. Two
# paragraphs is enough to satisfy it, and they go away again afterwards.
mkdir -p "$root/debian"
cat > "$root/debian/control" <<EOF
Source: gitrust

Package: gitrust
Architecture: $arch
EOF
linked=$( cd "$root" && dpkg-shlibdeps -O usr/bin/gitrust 2>/dev/null \
    | sed 's/^shlibs:Depends=//' )
rm -rf "$root/debian"
[[ -n $linked ]] || { echo "dpkg-shlibdeps said nothing about the binary"; exit 1; }

cat > "$root/DEBIAN/control" <<EOF
Package: gitrust
Version: $version
Section: devel
Priority: optional
Architecture: $arch
Depends: ${linked:+$linked, }$opened_at_runtime
Maintainer: Francesc Bagur <francesc.bagur@coffeesoftware.io>
Homepage: https://github.com/Coffee-Software-SLU/gitrust
Description: A desktop Git client built around the commit graph
 Draws a repository's whole history -- every branch and tag in one graph,
 with the merges entwined -- and lets you work in it: stage, commit, push,
 merge, rebase, and settle what will not merge on its own.
 .
 The board is not stored anywhere: its columns are read off the branches,
 so it travels with the repository and needs no account anywhere.
EOF

fakeroot dpkg-deb --build --root-owner-group "$root" "$out" >/dev/null
echo "$out"
dpkg-deb --info "$out" | sed -n '/Package:/,/^ *$/p'
