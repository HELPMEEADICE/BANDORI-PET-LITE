#!/usr/bin/env bash
# Build all release formats from the already compiled target executables.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=
VERSION=
for argument in "$@"; do
    case "$argument" in
        --target=*) TARGET="${argument#--target=}" ;;
        --version=*) VERSION="${argument#--version=}" ;;
        *) echo "Usage: $0 --target=<Rust target> --version=<version>" >&2; exit 2 ;;
    esac
done
case "$TARGET" in
    x86_64-unknown-linux-gnu) ARCH=x86_64; GOARCH=amd64; TRIPLET=x86_64-linux-gnu ;;
    aarch64-unknown-linux-gnu) ARCH=aarch64; GOARCH=arm64; TRIPLET=aarch64-linux-gnu ;;
    riscv64gc-unknown-linux-gnu) ARCH=riscv64; GOARCH=riscv64; TRIPLET=riscv64-linux-gnu ;;
    *) echo "Unsupported Linux target: $TARGET" >&2; exit 2 ;;
esac
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo 'Invalid package version' >&2; exit 2; }
cd "$ROOT"
bash scripts/package-linux.sh --skip-build --target="$TARGET"
OUTPUT="$ROOT/dist/linux-$TARGET-release"
PACKAGES="$OUTPUT/packages"
mkdir -p "$PACKAGES"
STAGING="$(mktemp -d "$OUTPUT/.release.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
NAME="BANDORI-PET-LITE-$VERSION-linux-$ARCH"
cp "$OUTPUT/BANDORI-PET-LITE.tar.gz" "$PACKAGES/$NAME.tar.gz"
tar -xzf "$OUTPUT/BANDORI-PET-LITE.tar.gz" -C "$STAGING"
export PACKAGE_PAYLOAD="$STAGING/BANDORI-PET-LITE"
export PACKAGE_ARCH="$GOARCH"
export PACKAGE_VERSION="$VERSION"
export PACKAGE_GLIBC_MIN
PACKAGE_GLIBC_MIN="$(getconf GNU_LIBC_VERSION | cut -d ' ' -f 2)"
export PACKAGE_GTK_DEB=libgtk-3-0
if dpkg-query -W -f='${Status}' "libgtk-3-0t64:$GOARCH" 2>/dev/null | grep -q 'install ok installed'; then
    PACKAGE_GTK_DEB=libgtk-3-0t64
fi

# nFPM runs on the build host and emits packages for the payload's architecture.
NFPM_VERSION=2.48.0
case "$(uname -m)" in
    x86_64) NFPM_ARCH=x86_64; NFPM_SHA=6d7f9a28b2f397934403950fcbba83f598b08d02edbe79a0815b3b4897341750 ;;
    aarch64) NFPM_ARCH=arm64; NFPM_SHA=a9a75213763783b7669719e505474c748c5ad7dc66591a195619deb50f85c2f4 ;;
    *) echo 'Unsupported packaging host' >&2; exit 2 ;;
esac
curl --fail --location --retry 3 \
    "https://github.com/goreleaser/nfpm/releases/download/v$NFPM_VERSION/nfpm_${NFPM_VERSION}_Linux_$NFPM_ARCH.tar.gz" \
    --output "$STAGING/nfpm.tar.gz"
echo "$NFPM_SHA  $STAGING/nfpm.tar.gz" | sha256sum --check --strict
tar -xzf "$STAGING/nfpm.tar.gz" -C "$STAGING" nfpm
"$STAGING/nfpm" package --config packaging/linux/nfpm.yaml --packager deb --target "$PACKAGES/$NAME.deb"
"$STAGING/nfpm" package --config packaging/linux/nfpm.yaml --packager rpm --target "$PACKAGES/$NAME.rpm"

APPDIR="$STAGING/BANDORI-PET-LITE.AppDir"
mkdir -p "$APPDIR/usr/lib/bandori-pet-lite" "$APPDIR/usr/share/doc/bandori-pet-lite"
cp -a "$PACKAGE_PAYLOAD/." "$APPDIR/usr/lib/bandori-pet-lite/"
cp packaging/linux/AppRun "$APPDIR/AppRun"
cp packaging/linux/bandori-pet.desktop "$APPDIR/bandori-pet.desktop"
# The AppImage desktop entry resolves the payload inside the mounted image.
sed -i 's|^Exec=.*|Exec=AppRun|' "$APPDIR/bandori-pet.desktop"
cp assets/icons/bandori.png "$APPDIR/bandori-pet.png"
ln -s bandori-pet.png "$APPDIR/.DirIcon"
chmod 755 "$APPDIR/AppRun"
python3 scripts/bundle-linux-libs.py "$APPDIR" "$TRIPLET"

# The official AppImage runtime has no riscv64 release. uruntime supports all
# three architectures and launches an appended SquashFS payload (no emulation).
RUNTIME_VERSION=0.8.1
case "$ARCH" in
    x86_64) RUNTIME_SHA=32c6024a14ff375749ee3bed3330c1586a6dd2bfeddbbd7ceacc053aec2b27a3 ;;
    aarch64) RUNTIME_SHA=5158dbe45224ecf51aa5ee46be22d4d63f30c6f13a7427bad4af0018016d836c ;;
    riscv64) RUNTIME_SHA=4639a923474d8dd43f988d60b7f3ad2d5d072c8a4c6f3f76dea493d5951b9b77 ;;
esac
curl --fail --location --retry 3 \
    "https://github.com/VHSgunzo/uruntime/releases/download/v$RUNTIME_VERSION/uruntime-appimage-squashfs-lite-$ARCH" \
    --output "$STAGING/runtime"
echo "$RUNTIME_SHA  $STAGING/runtime" | sha256sum --check --strict
curl --fail --location --retry 3 \
    "https://raw.githubusercontent.com/VHSgunzo/uruntime/v$RUNTIME_VERSION/LICENSE" \
    --output "$APPDIR/usr/share/doc/bandori-pet-lite/uruntime-LICENSE"
mksquashfs "$APPDIR" "$STAGING/payload.squashfs" -noappend -all-root -comp zstd
cat "$STAGING/runtime" "$STAGING/payload.squashfs" > "$PACKAGES/$NAME.AppImage"
chmod 755 "$PACKAGES/$NAME.AppImage"
# Extract and inspect the payload on native hosts; RISC-V remains cross-built.
if [[ "$(uname -m)" == "$ARCH" ]]; then
    (
        cd "$STAGING"
        "$PACKAGES/$NAME.AppImage" --appimage-extract
        test -x squashfs-root/AppRun
        test -x squashfs-root/usr/lib/bandori-pet-lite/bandori-pet
        test -x squashfs-root/usr/lib/bandori-pet-lite/bandori-ui
    )
fi
echo "Packages: $PACKAGES"
