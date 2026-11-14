@echo off
rem Piska Engine installer: double-click for the interactive menu, or pass options (-Components core,web -DryRun ...).
rem ExecutionPolicy Bypass applies to this one process only; no system setting is changed.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
if errorlevel 1 echo. & echo The installer stopped with an error; see the message above and docs\install.md.
if "%~1"=="" pause
