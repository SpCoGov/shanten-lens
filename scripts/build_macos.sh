#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
APP_DIR="${PROJECT_ROOT}/app"
PYTHON_BIN="${PROJECT_ROOT}/.venv/bin/python3"
BACKEND_ENTRY="${PROJECT_ROOT}/backend/run_server.py"
DIST_DIR="${PROJECT_ROOT}/dist"
BUILD_DIR="${PROJECT_ROOT}/build"
BACKEND_DIST="${DIST_DIR}/shanten-backend"
SIDECAR_DIR="${APP_DIR}/src-tauri/bin"
SIDECAR_BIN="${SIDECAR_DIR}/shanten-backend"
SIDECAR_BIN_EXE="${SIDECAR_DIR}/shanten-backend.exe"
TAURI_CONFIG="src-tauri/tauri.macos.conf.json"

log() {
  printf '\n[%s] %s\n' "$1" "$2"
}

fail() {
  printf '\n[ERROR] %s\n' "$1" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "Missing required command: $1"
}

log "INIT" "Project root: ${PROJECT_ROOT}"

require_cmd python3
require_cmd npm
require_cmd rustc
require_cmd cargo
require_cmd xcode-select

if ! xcode-select -p >/dev/null 2>&1; then
  fail "Xcode Command Line Tools are required. Run: xcode-select --install"
fi

if [[ ! -x "${PYTHON_BIN}" ]]; then
  fail "Missing virtualenv python at ${PYTHON_BIN}. Create it with: python3 -m venv .venv"
fi

log "1/5" "Installing Python build dependencies"
"${PYTHON_BIN}" -m ensurepip --upgrade >/dev/null 2>&1 || true
"${PYTHON_BIN}" -m pip install --upgrade pip wheel setuptools==80.9.0
"${PYTHON_BIN}" -m pip install -r "${PROJECT_ROOT}/requirements.txt"
"${PYTHON_BIN}" -m pip install pyinstaller

log "2/5" "Packaging Python backend with PyInstaller"
rm -rf "${BUILD_DIR}/shanten-backend" "${BACKEND_DIST}"
"${PYTHON_BIN}" -m PyInstaller \
  --noconfirm \
  --onefile \
  --name shanten-backend \
  --hidden-import uvicorn \
  --hidden-import fastapi \
  --hidden-import pydantic \
  --collect-all backend.data.assets \
  --add-data "${PROJECT_ROOT}/proto:proto" \
  "${BACKEND_ENTRY}"

[[ -f "${BACKEND_DIST}" ]] || fail "PyInstaller did not produce ${BACKEND_DIST}"

log "3/5" "Preparing Tauri sidecar"
mkdir -p "${SIDECAR_DIR}"
cp "${BACKEND_DIST}" "${SIDECAR_BIN}"
cp "${BACKEND_DIST}" "${SIDECAR_BIN_EXE}"
chmod +x "${SIDECAR_BIN}"
chmod +x "${SIDECAR_BIN_EXE}"

log "4/5" "Installing frontend dependencies and building app"
pushd "${APP_DIR}" >/dev/null
if [[ -f package-lock.json ]]; then
  npm ci
else
  npm install
fi
npm run build
npx tauri build --config "${TAURI_CONFIG}" --bundles app,dmg
popd >/dev/null

log "5/5" "Build finished"
printf 'App bundle: %s\n' "${APP_DIR}/src-tauri/target/release/bundle/macos/Shanten Lens.app"
printf 'DMG: %s\n' "${APP_DIR}/src-tauri/target/release/bundle/dmg/"
