#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE=release
SKIP_BUILD=0
TARGET=
for argument in "$@"; do
    case "$argument" in
        --debug) PROFILE=debug ;;
        --skip-build) SKIP_BUILD=1 ;;
        --target=*) TARGET="${argument#--target=}" ;;
        *) echo "Usage: $0 [--debug] [--skip-build] [--target=<Linux target>]" >&2; exit 2 ;;
    esac
done
if [[ "$(uname -s)" != Linux ]]; then
    echo "Run this script on Linux with the X11/GTK development dependencies installed." >&2
    exit 1
fi
if [[ -n "$TARGET" && "$TARGET" != *-unknown-linux-gnu ]]; then
    echo "Unsupported Linux target: $TARGET" >&2
    exit 2
fi
cd "$ROOT"
BUILD_ARGS=(--locked --bin bandori-pet --bin bandori-ui)
[[ "$PROFILE" == release ]] && BUILD_ARGS+=(--release)
[[ -n "$TARGET" ]] && BUILD_ARGS+=(--target "$TARGET")
if [[ "$SKIP_BUILD" == 0 ]]; then
    cargo build "${BUILD_ARGS[@]}"
fi
BUILD_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
[[ "$BUILD_DIR" = /* ]] || BUILD_DIR="$ROOT/$BUILD_DIR"
[[ -n "$TARGET" ]] && BUILD_DIR="$BUILD_DIR/$TARGET"
BIN_DIR="$BUILD_DIR/$PROFILE"
for binary in bandori-pet bandori-ui; do
    [[ -x "$BIN_DIR/$binary" ]] || { echo "Missing executable: $BIN_DIR/$binary" >&2; exit 1; }
done
ARCH="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
OUTPUT="$ROOT/dist/linux-$ARCH-$PROFILE"
mkdir -p "$OUTPUT"
STAGING="$(mktemp -d "$OUTPUT/.package.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
APP="$STAGING/BANDORI-PET-LITE"
mkdir -p "$APP/assets/icons"
cp "$BIN_DIR/bandori-pet" "$BIN_DIR/bandori-ui" "$APP/"
chmod 755 "$APP/bandori-pet" "$APP/bandori-ui"
cp "$ROOT/assets/band.json" "$ROOT/assets/outfit.json" "$APP/assets/"
cp "$ROOT/assets/icons/bandori.png" "$APP/assets/icons/"
cp "$ROOT/LICENSE" "$ROOT/NOTICE.md" "$ROOT/docs/LINUX.md" "$APP/"
tar -czf "$STAGING/BANDORI-PET-LITE.tar.gz" -C "$STAGING" BANDORI-PET-LITE
mv -f "$STAGING/BANDORI-PET-LITE.tar.gz" "$OUTPUT/BANDORI-PET-LITE.tar.gz"
echo "Created $OUTPUT/BANDORI-PET-LITE.tar.gz"
