#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../src-tauri"
stubs="$(mktemp -d)"
trap 'rm -rf "$stubs"' EXIT

cat > "$stubs/rc" <<'EOF'
#!/bin/sh
case "$*" in *"/?"*) echo "Resource Converter stub"; exit 0 ;; esac
out=""
while [ $# -gt 0 ]; do
  [ "$1" = "/fo" ] && { shift; out="$1"; }
  shift
done
[ -n "$out" ] && : > "$out"
exit 0
EOF

cat > "$stubs/pkg-config" <<'EOF'
#!/bin/sh
for arg in "$@"; do
  case "$arg" in
    --modversion) echo "99.0"; exit 0 ;;
    --version) echo "0.29.2"; exit 0 ;;
  esac
done
exit 0
EOF

chmod +x "$stubs/rc" "$stubs/pkg-config"
rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-gnu >/dev/null
export CARGO_TARGET_DIR=target/cross

echo "Windows:"
RC="$stubs/rc" cargo clippy --target x86_64-pc-windows-msvc -- -D warnings
echo "Linux:"
PKG_CONFIG="$stubs/pkg-config" PKG_CONFIG_ALLOW_CROSS=1 cargo clippy --target x86_64-unknown-linux-gnu -- -D warnings
