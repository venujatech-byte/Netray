@echo off
setlocal

echo ===================================================
echo             Netray Windows Build Script            
echo ===================================================

cd /d "%~dp0.."

echo [Netray] Compiling optimized release binary...
cargo build --release

if %ERRORLEVEL% equ 0 (
    echo [Netray] Build succeeded!
    echo [Netray] Binary location: target\release\netray-windows.exe
) else (
    echo [Netray] Build failed with error code %ERRORLEVEL%
    exit /b %ERRORLEVEL%
)

endlocal
