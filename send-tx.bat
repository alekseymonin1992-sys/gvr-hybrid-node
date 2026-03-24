@echo off
cd /d %~dp0

REM Отправка 10 GVR с кошелька "alekseymonin1992" на адрес "bob"
REM Нода должна быть запущена и слушать RPC на 127.0.0.1:8080

target\release\gvr-wallet.exe send ^
  --rpc 127.0.0.1:8080 ^
  --from-wallet alekseymonin1992 ^
  --to bob ^
  --amount 10 ^
  --fee 1

echo.
echo Done. Press any key to exit...
pause >nul