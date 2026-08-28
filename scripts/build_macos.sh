#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/.."

command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 1; }
command -v pnpm >/dev/null || { echo "pnpm is required" >&2; exit 1; }

echo "[1/2] Installing dependencies..."
cd app
pnpm install --frozen-lockfile

echo "[2/2] Building app and DMG with embedded Rust backend..."
pnpm exec tauri build --config src-tauri/tauri.macos.conf.json --bundles app,dmg
echo "Output: src-tauri/target/release/bundle/{macos,dmg}"
