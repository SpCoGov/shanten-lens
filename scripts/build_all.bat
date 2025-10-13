@echo off
setlocal EnableDelayedExpansion
chcp 65001 >nul
title Shanten Lens - One-Key Build (fixed pip)

REM === 进入项目根目录 ===
pushd "%~dp0\.."
set "PROJECT_ROOT=%CD%"
echo.
echo ===========================
echo  Shanten Lens - Build Start
echo ===========================
echo Project root: %PROJECT_ROOT%
echo.

REM === 路径与工具 ===
set "PYTHON=.venv\Scripts\python.exe"
set "APP_DIR=app"
set "BACKEND_ENTRY=backend\run_server.py"
set "DIST_EXE=dist\shanten-backend.exe"
set "SIDECAR_DIR=%APP_DIR%\src-tauri\bin"
set "SIDECAR_EXE=%SIDECAR_DIR%\shanten-backend.exe"
set "BUNDLE_DIR=%APP_DIR%\src-tauri\target\release\bundle"
set "PORTABLE_DIR=%BUNDLE_DIR%\windows\Shanten Lens"
set "PORTABLE_ZIP=%PROJECT_ROOT%\Shanten-Lens-portable.zip"

REM --- 基础检查 ---
if not exist "%PYTHON%" (
  echo [ERROR] venv Python 未找到: %PYTHON%
  echo 請先創建並安裝依賴：py -3 -m venv .venv ^& .venv\Scripts\python.exe -m pip install -r requirements.txt
  popd & exit /b 1
)
where npm >nul 2>nul || (echo [ERROR] Node.js/npm 未安装或未在 PATH 中 & popd & exit /b 1)
where rustc >nul 2>nul || (echo [ERROR] Rust 未安装或未在 PATH 中（需要 rustup） & popd & exit /b 1)
where powershell >nul 2>nul || (echo [ERROR] 需要 PowerShell（用于 ZIP） & popd & exit /b 1)

echo.
echo [1/6] 檢查/修復 pip...
"%PYTHON%" -m ensurepip --upgrade >nul 2>nul
"%PYTHON%" -m pip --version || (echo [ERROR] pip 不可用 & popd & exit /b 1)
"%PYTHON%" -m pip install --upgrade pip setuptools wheel || (echo [WARN] 升級 pip/工具鏈失敗，繼續嘗試)

echo.
echo [2/6] 安裝後端依賴（python -m pip）...
"%PYTHON%" -m pip install -r requirements.txt || (echo [ERROR] pip install 失敗 & popd & exit /b 1)
"%PYTHON%" -m pip install pyinstaller || (echo [ERROR] 安裝 pyinstaller 失敗 & popd & exit /b 1)

echo.
echo [3/6] 打包後端（PyInstaller）...
if exist "%DIST_EXE%" del /q "%DIST_EXE%" >nul 2>nul

REM 把 proto 整個資料夾打進 EXE（Windows 用 ; 分隔 src;dest）
"%PYTHON%" -m PyInstaller ^
  --noconfirm ^
  --onefile ^
  --noconsole ^
  --name shanten-backend ^
  --hidden-import uvicorn ^
  --hidden-import fastapi ^
  --hidden-import pydantic ^
  --add-data "proto;proto" ^
  "%BACKEND_ENTRY%"
if errorlevel 1 (echo [ERROR] PyInstaller 打包失敗 & popd & exit /b 1)

if not exist "%DIST_EXE%" (
  echo [ERROR] 未找到生成的後端 EXE: %DIST_EXE%
  popd & exit /b 1
)
echo 打包完成: %DIST_EXE%

echo.
echo [4/6] 部署 sidecar 到 Tauri...
if not exist "%SIDECAR_DIR%" mkdir "%SIDECAR_DIR%"
copy /y "%DIST_EXE%" "%SIDECAR_EXE%" >nul || (echo [ERROR] 複製 sidecar 失敗 & popd & exit /b 1)
echo 已複製: %SIDECAR_EXE%

echo.
echo [5/6] 構建前端與 Tauri...
pushd "%APP_DIR%"
if exist package-lock.json (
  call npm ci || (echo [ERROR] npm ci 失敗 & popd & popd & exit /b 1)
) else (
  call npm install || (echo [ERROR] npm install 失敗 & popd & popd & exit /b 1)
)
call npm run build || (echo [ERROR] 前端 vite 構建失敗 & popd & popd & exit /b 1)
call npm run tauri:build || (echo [ERROR] tauri 打包失敗 & popd & popd & exit /b 1)
popd

echo.
echo [6/6] 生成綠色版 ZIP...
set "PRODUCT_NAME=Shanten Lens"
set "BUNDLE_ROOT=%APP_DIR%\src-tauri\target\release\bundle"
set "RELEASE_EXE=%APP_DIR%\src-tauri\target\release\shanten-lens.exe"
set "SIDECAR_SRC=%APP_DIR%\src-tauri\bin\shanten-backend.exe"
set "PORTABLE_OUT=%APP_DIR%\src-tauri\target\release\portable\%PRODUCT_NAME%"

if not exist "%RELEASE_EXE%" (
  echo [ERROR] 未找到 release exe: %RELEASE_EXE%
  goto :SHOWPATHS
)
if not exist "%SIDECAR_SRC%" (
  echo [ERROR] 未找到 sidecar: %SIDECAR_SRC%
  goto :SHOWPATHS
)

if exist "%PORTABLE_OUT%" rmdir /s /q "%PORTABLE_OUT%"
mkdir "%PORTABLE_OUT%\resources\bin" 2>nul

copy /y "%RELEASE_EXE%" "%PORTABLE_OUT%\" >nul || (echo [ERROR] 複製主程序失敗 & goto :SHOWPATHS)
copy /y "%SIDECAR_SRC%" "%PORTABLE_OUT%\resources\bin\shanten-backend.exe" >nul || (echo [ERROR] 複製 sidecar 失敗 & goto :SHOWPATHS)

> "%PORTABLE_OUT%\README.txt" echo Portable build of %PRODUCT_NAME%. Double-click "shanten-lens.exe" to run.

if exist "%PORTABLE_ZIP%" del /q "%PORTABLE_ZIP%" >nul 2>nul
powershell -NoProfile -Command "Compress-Archive -Path '%PORTABLE_OUT%\*' -DestinationPath '%PORTABLE_ZIP%' -Force"
if exist "%PORTABLE_ZIP%" (
  echo 已生成綠色版: %PORTABLE_ZIP%
) else (
  echo [WARN] PowerShell 壓縮失敗，嘗試 tar.exe...
  where tar >nul 2>nul
  if errorlevel 1 (
    echo [ERROR] 沒有 tar.exe，請手動用 7-Zip 壓縮: %PORTABLE_OUT%
  ) else (
    pushd "%PORTABLE_OUT%"
    tar.exe -a -c -f "%PORTABLE_ZIP%" *
    popd
    if exist "%PORTABLE_ZIP%" (
      echo 已生成綠色版（tar）：%PORTABLE_ZIP%
    ) else (
      echo [ERROR] ZIP 壓縮失敗（PowerShell 與 tar 均失敗）
    )
  )
)

:SHOWPATHS
echo.
echo ===========================
echo  Build Finished
echo ===========================
echo 綠色版目錄: "%PORTABLE_OUT%"
echo 安裝包目錄: "%BUNDLE_ROOT%"
echo.

popd