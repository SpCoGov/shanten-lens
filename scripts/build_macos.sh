#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/.."

command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 1; }
command -v pnpm >/dev/null || { echo "pnpm is required" >&2; exit 1; }

rustup target add aarch64-apple-darwin x86_64-apple-darwin

echo "[1/2] Installing dependencies..."
cd app
pnpm install --frozen-lockfile

echo "[2/2] Building universal app and DMG with embedded Rust backend..."
pnpm exec tauri build --config src-tauri/tauri.macos.conf.json --target universal-apple-darwin --bundles app,dmg
bundle="src-tauri/target/universal-apple-darwin/release/bundle"
app_bundle="$bundle/macos/Shanten Lens.app"
executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app_bundle/Contents/Info.plist")
lipo "$app_bundle/Contents/MacOS/$executable" -verify_arch x86_64 arm64
codesign --verify --deep --strict "$app_bundle"
echo "Output: $bundle/{macos,dmg}"
