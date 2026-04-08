@echo off
setlocal EnableExtensions EnableDelayedExpansion
chcp 65001 >nul
    title Shanten Lens - One-Key Build

pushd "%~dp0\.." || exit /b 1
set "PROJECT_ROOT=%CD%"

echo.
echo ===========================
echo  Shanten Lens - Build Start
echo ===========================
echo Project root: %PROJECT_ROOT%
echo.

set "PYTHON=.venv\Scripts\python.exe"
set "APP_DIR=app"
set "BACKEND_ENTRY=backend\run_server.py"
set "DIST_EXE=dist\shanten-backend.exe"
set "SIDECAR_DIR=%APP_DIR%\src-tauri\bin"
set "SIDECAR_EXE=%SIDECAR_DIR%\shanten-backend.exe"
set "BUNDLE_DIR=%APP_DIR%\src-tauri\target\release\bundle"
set "PORTABLE_ZIP=%PROJECT_ROOT%\Shanten-Lens-portable.zip"
set "PRODUCT_NAME=Shanten Lens"
set "RELEASE_EXE=%APP_DIR%\src-tauri\target\release\shanten-lens.exe"
set "SIDECAR_SRC=%APP_DIR%\src-tauri\bin\shanten-backend.exe"
set "PORTABLE_OUT=%APP_DIR%\src-tauri\target\release\portable\%PRODUCT_NAME%"

if not exist "%PYTHON%" (
  echo [ERROR] venv python not found: %PYTHON%
  popd
  exit /b 1
)

where npm >nul 2>nul || (
  echo [ERROR] Node.js/npm not found in PATH.
  popd
  exit /b 1
)

where rustc >nul 2>nul || (
  echo [ERROR] rustc not found in PATH.
  popd
  exit /b 1
)

where powershell >nul 2>nul || (
  echo [ERROR] powershell is required.
  popd
  exit /b 1
)

echo.
echo [1/6] Checking pip...
"%PYTHON%" -m ensurepip --upgrade >nul 2>nul
"%PYTHON%" -m pip --version >nul || (
  echo [ERROR] pip is not available.
  popd
  exit /b 1
)
"%PYTHON%" -m pip install --upgrade pip wheel || echo [WARN] pip/wheel upgrade failed, continue...
"%PYTHON%" -m pip install --upgrade --force-reinstall setuptools==80.9.0 || (
  echo [ERROR] setuptools install failed.
  popd
  exit /b 1
)

echo.
echo [2/6] Installing backend dependencies...
"%PYTHON%" -m pip install -r requirements.txt || (
  echo [ERROR] pip install -r requirements.txt failed.
  popd
  exit /b 1
)
"%PYTHON%" -m pip install pyinstaller || (
  echo [ERROR] pip install pyinstaller failed.
  popd
  exit /b 1
)

echo.
echo [3/6] Building backend with PyInstaller...
if exist "%DIST_EXE%" del /q "%DIST_EXE%" >nul 2>nul
"%PYTHON%" -m PyInstaller --noconfirm --onefile --noconsole --name shanten-backend --collect-all backend.data.assets --add-data "proto;proto" "%BACKEND_ENTRY%" || (
  echo [ERROR] PyInstaller build failed.
  popd
  exit /b 1
)
if not exist "%DIST_EXE%" (
  echo [ERROR] Backend exe not found: %DIST_EXE%
  popd
  exit /b 1
)
echo Built: %DIST_EXE%

echo.
echo [4/6] Copying sidecar to Tauri...
if not exist "%SIDECAR_DIR%" mkdir "%SIDECAR_DIR%"
copy /y "%DIST_EXE%" "%SIDECAR_EXE%" >nul || (
  echo [ERROR] Failed to copy sidecar.
  popd
  exit /b 1
)
echo Copied: %SIDECAR_EXE%

echo.
echo [5/6] Building frontend and Tauri...
pushd "%APP_DIR%" || (
  echo [ERROR] Cannot enter app directory.
  popd
  exit /b 1
)
if exist package-lock.json (
  call npm ci || (
    echo [ERROR] npm ci failed.
    popd
    popd
    exit /b 1
  )
) else (
  call npm install || (
    echo [ERROR] npm install failed.
    popd
    popd
    exit /b 1
  )
)
call npm run build || (
  echo [ERROR] frontend build failed.
  popd
  popd
  exit /b 1
)
call npm run tauri:build || (
  echo [ERROR] tauri build failed.
  popd
  popd
  exit /b 1
)
popd

echo.
echo [6/6] Creating portable ZIP...
if not exist "%RELEASE_EXE%" (
  echo [ERROR] Release exe not found: %RELEASE_EXE%
  goto :SHOWPATHS
)
if not exist "%SIDECAR_SRC%" (
  echo [ERROR] Sidecar exe not found: %SIDECAR_SRC%
  goto :SHOWPATHS
)

if exist "%PORTABLE_OUT%" rmdir /s /q "%PORTABLE_OUT%"
mkdir "%PORTABLE_OUT%\resources\bin" 2>nul

copy /y "%RELEASE_EXE%" "%PORTABLE_OUT%\shanten-lens.exe" >nul || (
  echo [ERROR] Failed to copy main exe.
  goto :SHOWPATHS
)
copy /y "%SIDECAR_SRC%" "%PORTABLE_OUT%\resources\bin\shanten-backend.exe" >nul || (
  echo [ERROR] Failed to copy sidecar exe.
  goto :SHOWPATHS
)

echo Portable build of %PRODUCT_NAME%. Double-click shanten-lens.exe to run.>"%PORTABLE_OUT%\README.txt"

if exist "%PORTABLE_ZIP%" del /q "%PORTABLE_ZIP%" >nul 2>nul
powershell -NoProfile -Command "Compress-Archive -Path '%PORTABLE_OUT%\*' -DestinationPath '%PORTABLE_ZIP%' -Force"

if exist "%PORTABLE_ZIP%" (
  echo Portable ZIP created: %PORTABLE_ZIP%
) else (
  echo [WARN] PowerShell zip failed. Trying tar.exe...
  where tar >nul 2>nul
  if errorlevel 1 (
    echo [ERROR] tar.exe not found. Please zip manually: %PORTABLE_OUT%
  ) else (
    pushd "%PORTABLE_OUT%"
    tar.exe -a -c -f "%PORTABLE_ZIP%" *
    popd
    if exist "%PORTABLE_ZIP%" (
      echo Portable ZIP created with tar: %PORTABLE_ZIP%
    ) else (
      echo [ERROR] ZIP creation failed ^(PowerShell and tar both failed^).
    )
  )
)

:SHOWPATHS
echo.
echo ===========================
echo  Build Finished
echo ===========================
echo Portable dir: "%PORTABLE_OUT%"
echo Bundle dir:   "%BUNDLE_DIR%"
echo.

popd
