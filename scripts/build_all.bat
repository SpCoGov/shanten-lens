@echo off
setlocal EnableExtensions
chcp 65001 >nul

where cargo >nul 2>nul || (echo [ERROR] cargo not found & exit /b 1)
where pnpm >nul 2>nul || (echo [ERROR] pnpm not found & exit /b 1)
if not defined TAURI_UPDATER_PUBLIC_KEY (echo [ERROR] TAURI_UPDATER_PUBLIC_KEY is required for automatic updates & exit /b 1)
if not defined TAURI_SIGNING_PRIVATE_KEY (echo [ERROR] TAURI_SIGNING_PRIVATE_KEY is required to sign updates & exit /b 1)

pushd "%~dp0\..\app" || exit /b 1

echo [1/2] Installing dependencies...
call pnpm install --frozen-lockfile
if errorlevel 1 goto :fail

echo [2/2] Building signed NSIS installer with embedded Rust backend...
node -e "const fs=require('fs');fs.mkdirSync('../build',{recursive:true});fs.writeFileSync('../build/tauri-updater.json',JSON.stringify({plugins:{updater:{pubkey:process.env.TAURI_UPDATER_PUBLIC_KEY.trim()}}}));"
if errorlevel 1 goto :fail
call pnpm run tauri:build --config ../build/tauri-updater.json
if errorlevel 1 goto :fail

popd
echo Output: app\src-tauri\target\release\bundle\nsis
exit /b 0

:fail
set "BUILD_EXIT=%ERRORLEVEL%"
popd
exit /b %BUILD_EXIT%
