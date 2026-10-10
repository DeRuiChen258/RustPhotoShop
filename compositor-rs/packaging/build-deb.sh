#!/usr/bin/env bash
# Builds a .deb for the current machine's architecture from the release binary.
#
# Installs into /usr (binary, desktop entry, icons, MIME type), which is what a system-wide
# package should do. For a single user, `scripts/install.sh` needs no root at all.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"

version="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
architecture="$(dpkg --print-architecture)"
package="rustphotoshop"
build="$here/target/deb/${package}_${version}_${architecture}"
binary="$here/target/release/rustphotoshop"

if [[ ! -x "$binary" ]]; then
  echo "先运行 scripts/build.sh" >&2
  exit 1
fi

rm -rf "$build"
mkdir -p "$build/DEBIAN" \
         "$build/usr/bin" \
         "$build/usr/share/applications" \
         "$build/usr/share/rustphotoshop/assets" \
         "$build/usr/share/mime/packages"

install -m 0755 "$binary" "$build/usr/bin/$package"

# Icons, in the hicolor theme's own layout.
for size in 16 32 48 64 128 256 512; do
  source="$here/assets/icons/hicolor/${size}x${size}/apps/rustphotoshop.png"
  target="$build/usr/share/icons/hicolor/${size}x${size}/apps"
  [[ -f "$source" ]] || continue
  mkdir -p "$target"
  install -m 0644 "$source" "$target/rustphotoshop.png"
done

# The assets the running application reads, and the MIME definition for .comp packages.
cp -r "$here/assets/icons" "$build/usr/share/rustphotoshop/assets/icons"
cp -r "$here/assets/mime" "$build/usr/share/rustphotoshop/assets/mime"
install -m 0644 "$here/assets/mime/application-x-compositor-project.xml" \
  "$build/usr/share/mime/packages/application-x-compositor-project.xml"

# The desktop entry, with the system-wide path filled in.
sed -e 's|^Exec=rustphotoshop |Exec=/usr/bin/rustphotoshop |' \
    -e 's|^TryExec=.*|TryExec=/usr/bin/rustphotoshop|' \
    "$here/assets/rustphotoshop.desktop" >"$build/usr/share/applications/rustphotoshop.desktop"
chmod 0644 "$build/usr/share/applications/rustphotoshop.desktop"

installed_size="$(du -sk "$build/usr" | cut -f1)"
cat >"$build/DEBIAN/control" <<CONTROL
Package: $package
Version: $version
Section: graphics
Priority: optional
Architecture: $architecture
Installed-Size: $installed_size
Depends: libc6, libgcc-s1, libgl1, libxkbcommon0
Recommends: fonts-noto-cjk
Maintainer: RustPhotoShop Linux port <noreply@example.invalid>
Description: Open-source image editor for compositing and photo work
 RustPhotoShop is an image editor built around a Photoshop-like workflow:
 layers with blend modes and masks, selections, painting and retouching,
 adjustments and filters, and the .comp project format it shares with the
 macOS build (which is a folder of PNG layers plus a manifest.json).
CONTROL

cat >"$build/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true
fi
if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database /usr/share/mime || true
fi
exit 0
POSTINST
chmod 0755 "$build/DEBIAN/postinst"

cat >"$build/DEBIAN/postrm" <<'POSTRM'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database /usr/share/applications || true
fi
exit 0
POSTRM
chmod 0755 "$build/DEBIAN/postrm"

output="$here/target/deb/${package}_${version}_${architecture}.deb"
dpkg-deb --build --root-owner-group "$build" "$output"
echo
echo "已生成：$output"
dpkg-deb --info "$output" | sed -n '1,12p'
