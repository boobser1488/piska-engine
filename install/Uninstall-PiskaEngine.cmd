@echo off
rem Removes only what the Piska Engine installer added (it asks before every program it installed).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" -Uninstall %*
pause
