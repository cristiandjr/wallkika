#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
version="${1:?usage: build-saver.sh <version>}"
src=screensaver/macos
out=src-tauri/target/screensaver/WallKika.saver
binary="$out/Contents/MacOS/WallKika"

if [ -f "$binary" ] && grep -q "<string>$version</string>" "$out/Contents/Info.plist" \
  && [ -z "$(find "$src" scripts/build-saver.sh -newer "$binary" | head -1)" ]; then
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
for arch in arm64 x86_64; do
  swiftc -O -swift-version 5 -module-name WallKikaSaver -target "$arch-apple-macos13.0" \
    -emit-library -Xlinker -bundle -o "$work/WallKika-$arch" "$src/WallKikaSaver.swift"
done

rm -rf "$out"
mkdir -p "$out/Contents/MacOS" "$out/Contents/Resources"
lipo -create "$work"/WallKika-* -output "$binary"
sed "s/@VERSION@/$version/g" "$src/Info.plist" > "$out/Contents/Info.plist"
cp "$src"/thumbnail*.png "$out/Contents/Resources/"
codesign --force --sign - "$out"
echo "Built $out ($version)"
