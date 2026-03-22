@echo off
cd /d %~dp0

echo GVR Client: status
echo.

client.exe status --rpc 127.0.0.1:8080

pause
