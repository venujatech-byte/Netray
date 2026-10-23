@echo off
setlocal

echo [Netray] Starting Netray for Windows...

:: Check if Netray executable exists in release or debug
if exist "%~dp0..\target\release\netray-windows.exe" (
    start "" "%~dp0..\target\release\netray-windows.exe"
    exit /b 0
)

if exist "%~dp0..\target\debug\netray-windows.exe" (
    start "" "%~dp0..\target\debug\netray-windows.exe"
    exit /b 0
)

echo [Netray] Executable not found. Compiling with cargo run --release...
cargo run --manifest-path "%~dp0..\Cargo.toml" --release
endlocal
