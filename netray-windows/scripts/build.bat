@echo off
setlocal

echo ===================================================
echo             Netray Windows Build Script            
echo ===================================================

cd /d "%~dp0.."

echo [Netray] Compiling optimized release binary...
cargo build --release

if %ERRORLEVEL% equ 0 (
    echo [Netray] Build succeeded. Creating portable release package...
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package-windows.ps1
    if errorlevel 1 exit /b 1
) else (
    echo [Netray] Build failed with error code %ERRORLEVEL%
    exit /b %ERRORLEVEL%
)

endlocal
