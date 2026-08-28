@echo off
setlocal EnableExtensions
chcp 65001 >nul

where cargo >nul 2>nul || (echo [ERROR] cargo not found & exit /b 1)
where pnpm >nul 2>nul || (echo [ERROR] pnpm not found & exit /b 1)

pushd "%~dp0\..\app" || exit /b 1

echo [1/2] Installing dependencies...
call pnpm install --frozen-lockfile
if errorlevel 1 goto :fail

echo [2/2] Building MSI with embedded Rust backend...
call pnpm run tauri:build
if errorlevel 1 goto :fail

popd
echo Output: app\src-tauri\target\release\bundle\msi
exit /b 0

:fail
set "BUILD_EXIT=%ERRORLEVEL%"
popd
exit /b %BUILD_EXIT%
