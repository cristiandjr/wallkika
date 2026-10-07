#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
out=release
rm -rf "$out"
mkdir -p "$out"

case "$(uname -s)" in
  Darwin)
    target="${MAC_TARGET:-universal-apple-darwin}"
    tools="$(mktemp -d)"
    trap 'rm -rf "$tools"' EXIT
    # Tauri needs Apple's xattr (-c/-r); the pip "xattr" package can shadow it on PATH.
    ln -s /usr/bin/xattr "$tools/xattr"
    PATH="$tools:$PATH" npm run tauri build -- --target "$target" --bundles app
    ditto -c -k --keepParent "src-tauri/target/$target/release/bundle/macos/WallKika.app" "$out/WallKika-macOS.zip"
    ;;
  Linux)
    npm run tauri build -- --bundles appimage
    cp src-tauri/target/release/bundle/appimage/*.AppImage "$out/WallKika-Linux-x86_64.AppImage"
    chmod +x "$out/WallKika-Linux-x86_64.AppImage"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    npm run tauri build -- --no-bundle
    cp src-tauri/target/release/wallkika.exe "$out/WallKika-Windows-x64.exe"
    ;;
  *)
    echo "Unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

ls -lh "$out"
