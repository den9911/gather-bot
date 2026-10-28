@echo off
cd /d "%~dp0"
where cargo >nul 2>&1 || (echo Install Rust from https://rustup.rs/ and restart this terminal. & exit /b 1)
if "%TELOXIDE_TOKEN%"=="" (echo Set TELOXIDE_TOKEN in this terminal first. & exit /b 1)
cargo run --release
pause
