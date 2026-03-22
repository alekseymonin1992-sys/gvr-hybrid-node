@echo off
cd /d %~dp0

echo GVR Wallet: send from alice to bob (amount=10, fee=1)
echo.

wallet.exe send --from-wallet alice --to bob --amount 10 --fee 1 --rpc 127.0.0.1:8080

pause
