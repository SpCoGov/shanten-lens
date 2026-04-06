#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
pushd "${SCRIPT_DIR}/.." >/dev/null
PROJECT_ROOT="$(pwd)"

echo
echo "==========================="
echo " Shanten Lens - Build Start"
echo "==========================="
echo "Project root: ${PROJECT_ROOT}"
echo

PYTHON="${PROJECT_ROOT}/.venv/bin/python3"
APP_DIR="${PROJECT_ROOT}/app"
BACKEND_ENTRY="${PROJECT_ROOT}/backend/run_server.py"
DIST_BIN="${PROJECT_ROOT}/dist/shanten-backend"
BUILD_WORK_DIR="${PROJECT_ROOT}/build/shanten-backend"
SIDECAR_DIR="${APP_DIR}/src-tauri/bin"
SIDECAR_BIN="${SIDECAR_DIR}/shanten-backend"
SIDECAR_BIN_EXE="${SIDECAR_DIR}/shanten-backend.exe"
TAURI_CONFIG="src-tauri/tauri.macos.conf.json"
BUNDLE_DIR="${APP_DIR}/src-tauri/target/release/bundle"
APP_BUNDLE="${BUNDLE_DIR}/macos/Shanten Lens.app"
DMG_DIR="${BUNDLE_DIR}/dmg"

fail() {
  echo "[ERROR] $1" >&2
  popd >/dev/null
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "$2"
}

if [[ ! -x "${PYTHON}" ]]; then
  fail "venv Python not found: ${PYTHON}"
fi
require_cmd npm "Node.js/npm is not installed or not in PATH"
require_cmd rustc "Rust is not installed or not in PATH (install via rustup)"
require_cmd cargo "cargo is not installed or not in PATH"
require_cmd xcode-select "xcode-select is not available"

if ! xcode-select -p >/dev/null 2>&1; then
  fail "Xcode Command Line Tools are required. Run: xcode-select --install"
fi

echo
echo "[1/6] Checking/fixing pip..."
"${PYTHON}" -m ensurepip --upgrade >/dev/null 2>&1 || true
"${PYTHON}" -m pip --version >/dev/null || fail "pip is not available"
"${PYTHON}" -m pip install --upgrade pip wheel || echo "[WARN] Failed to upgrade pip/wheel, continuing..."
echo "[INFO] Pinning setuptools to avoid pkg_resources issues..."
"${PYTHON}" -m pip install --upgrade --force-reinstall setuptools==80.9.0 || fail "Failed to install setuptools==80.9.0"
echo "[INFO] setuptools pin complete"

echo
echo "[2/6] Installing backend dependencies (python -m pip)..."
"${PYTHON}" -m pip install -r "${PROJECT_ROOT}/requirements.txt" || fail "pip install -r requirements.txt failed"
"${PYTHON}" -m pip install pyinstaller || fail "Installing pyinstaller failed"

echo
echo "[3/6] Packaging backend (PyInstaller)..."
rm -rf "${BUILD_WORK_DIR}" "${DIST_BIN}"
"${PYTHON}" -m PyInstaller \
  --noconfirm \
  --onefile \
  --name shanten-backend \
  --hidden-import uvicorn \
  --hidden-import fastapi \
  --hidden-import pydantic \
  --collect-all backend.data.assets \
  --add-data "${PROJECT_ROOT}/proto:proto" \
  "${BACKEND_ENTRY}" || fail "PyInstaller build failed"

[[ -f "${DIST_BIN}" ]] || fail "Generated backend binary not found: ${DIST_BIN}"
echo "Packaging complete: ${DIST_BIN}"

echo
echo "[4/6] Deploying sidecar to Tauri..."
mkdir -p "${SIDECAR_DIR}"
cp "${DIST_BIN}" "${SIDECAR_BIN}" || fail "Copying sidecar failed"
cp "${DIST_BIN}" "${SIDECAR_BIN_EXE}" || fail "Copying compatibility sidecar failed"
chmod +x "${SIDECAR_BIN}"
chmod +x "${SIDECAR_BIN_EXE}"
echo "Prepared: ${SIDECAR_BIN}"

echo
echo "[5/6] Building frontend and Tauri..."
pushd "${APP_DIR}" >/dev/null
if [[ -f package-lock.json ]]; then
  npm ci || { popd >/dev/null; fail "npm ci failed"; }
else
  npm install || { popd >/dev/null; fail "npm install failed"; }
fi
npm run build || { popd >/dev/null; fail "Frontend vite build failed"; }
npx tauri build --config "${TAURI_CONFIG}" --bundles app,dmg || { popd >/dev/null; fail "tauri build failed"; }
popd >/dev/null

echo
echo "[6/6] Checking macOS bundle outputs..."
[[ -d "${APP_BUNDLE}" ]] || fail "App bundle not found: ${APP_BUNDLE}"
[[ -d "${DMG_DIR}" ]] || echo "[WARN] DMG output directory not found yet: ${DMG_DIR}"

echo
echo "==========================="
echo " Build Finished"
echo "==========================="
echo "App bundle: ${APP_BUNDLE}"
echo "DMG directory: ${DMG_DIR}"
echo

popd >/dev/null
