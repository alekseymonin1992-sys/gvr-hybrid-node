@echo off
cd /d %~dp0

echo GVR Wallet: create wallet 'alice'
echo.

wallet.exe new --name alice

pause
