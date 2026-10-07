#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
out="$PWD/release"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
rm -rf "$out"
mkdir -p "$out"

case "$(uname -s)" in
  Darwin)
    target="${MAC_TARGET:-universal-apple-darwin}"
    # Tauri needs Apple's xattr (-c/-r); the pip "xattr" package can shadow it on PATH.
    ln -s /usr/bin/xattr "$stage/xattr"
    PATH="$stage:$PATH" npm run tauri build -- --target "$target" --bundles app
    ditto -c -k --keepParent "src-tauri/target/$target/release/bundle/macos/WallKika.app" "$out/WallKika-macOS.zip"
    ;;
  Linux)
    npm run tauri build -- --bundles appimage
    cp src-tauri/target/release/bundle/appimage/*.AppImage "$stage/WallKika.AppImage"
    chmod +x "$stage/WallKika.AppImage"
    (cd "$stage" && zip -q -9 "$out/WallKika-Linux-x86_64.zip" WallKika.AppImage)
    ;;
  MINGW* | MSYS* | CYGWIN*)
    npm run tauri build -- --no-bundle
    cp src-tauri/target/release/wallkika.exe "$stage/WallKika.exe"
    powershell.exe -NoProfile -Command "Compress-Archive -Path '$(cygpath -w "$stage/WallKika.exe")' -DestinationPath '$(cygpath -w "$out/WallKika-Windows-x64.zip")' -Force"
    ;;
  *)
    echo "Unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

ls -lh "$out"
