#!/usr/bin/env bash
# Build a relocatable macOS .app containing the pet and its UI companion.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE=release
TARGET=""
SKIP_BUILD=0
for arg in "$@"; do
    case "$arg" in
        --debug) PROFILE=debug ;;
        --skip-build) SKIP_BUILD=1 ;;
        --target=*) TARGET="${arg#--target=}" ;;
        *) echo "Usage: $0 [--debug] [--skip-build] [--target=aarch64-apple-darwin|x86_64-apple-darwin]" >&2; exit 2 ;;
    esac
done
if [[ "$(uname -s)" != Darwin ]]; then
    echo "Run this script on macOS (Xcode command line tools are required)." >&2
    exit 1
fi
if [[ -n "$TARGET" && "$TARGET" != aarch64-apple-darwin && "$TARGET" != x86_64-apple-darwin ]]; then
    echo "Unsupported macOS target: $TARGET" >&2
    exit 2
fi
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"
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
VERSION="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "bandori-pet-bin"))')"
ARCH="${TARGET:-$(uname -m)}"
OUTPUT="$ROOT/dist/macos-$ARCH-$PROFILE"
mkdir -p "$OUTPUT"
# Assemble in a fresh directory; replace only the generated bundle at the end.
STAGING="$(mktemp -d "$OUTPUT/.bundle.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
APP="$STAGING/BANDORI-PET-LITE.app"
CONTENTS="$APP/Contents"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources/assets"
cp "$BIN_DIR/bandori-pet" "$BIN_DIR/bandori-ui" "$CONTENTS/MacOS/"
chmod 755 "$CONTENTS/MacOS/bandori-pet" "$CONTENTS/MacOS/bandori-ui"
cp "$ROOT/assets/band.json" "$ROOT/assets/outfit.json" "$CONTENTS/Resources/assets/"
cp "$ROOT/LICENSE" "$ROOT/NOTICE.md" "$CONTENTS/Resources/"

# Use the existing project icon at all standard Retina sizes.
ICONSET="$STAGING/bandori.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$ROOT/assets/icons/bandori.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    retina=$((size * 2))
    sips -z "$retina" "$retina" "$ROOT/assets/icons/bandori.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$CONTENTS/Resources/bandori.icns"
cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>io.github.bandori-pet-lite</string>
<key>CFBundleName</key><string>BANDORI-PET-LITE</string>
<key>CFBundleDisplayName</key><string>BANDORI-PET-LITE</string>
<key>CFBundleExecutable</key><string>bandori-pet</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>bandori.icns</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$VERSION</string>
<key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
<key>LSUIElement</key><true/>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
plutil -lint "$CONTENTS/Info.plist"
# Ad-hoc signing makes locally built Apple Silicon bundles launchable. Public
# distribution still needs Developer ID signing and Apple notarization.
codesign --force --sign - "$CONTENTS/MacOS/bandori-ui"
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"
DESTINATION="$OUTPUT/BANDORI-PET-LITE.app"
if [[ -e "$DESTINATION" ]]; then
    mv "$DESTINATION" "$STAGING/previous.app"
fi
mv "$APP" "$DESTINATION"
ditto -c -k --sequesterRsrc --keepParent "$DESTINATION" "$OUTPUT/BANDORI-PET-LITE.zip"
echo "App: $DESTINATION"
echo "Archive: $OUTPUT/BANDORI-PET-LITE.zip"
