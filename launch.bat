@echo off
rem Launches the game with the mod (customization-plus.me3, next to this file) through me3 from the PATH.
chcp 65001 >nul
cd /d "%~dp0"
where me3 >nul 2>nul || (
    echo me3 isn't on the PATH. Install it from https://github.com/garyttierney/me3, or open
    echo customization-plus.me3 with it.
    pause
    exit /b 1
)
me3 launch -g armoredcore6 -p customization-plus.me3
if errorlevel 1 pause
