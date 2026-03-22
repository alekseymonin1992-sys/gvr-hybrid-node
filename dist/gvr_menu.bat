@echo off
cd /d %~dp0

:menu
cls
echo =========================================
echo         GVR Hybrid Node - MENU
echo =========================================
echo 1) Start node
echo 2) Show network status
echo 3) Create wallet (wallet new)
echo 4) Show wallet info (wallet show)
echo 5) Send transaction (wallet send)
echo 0) Exit
echo =========================================
set /p choice=Select option (0-5): 

if "%choice%"=="1" goto run_node
if "%choice%"=="2" goto status
if "%choice%"=="3" goto wallet_new
if "%choice%"=="4" goto wallet_show
if "%choice%"=="5" goto wallet_send
if "%choice%"=="0" goto end

echo Invalid choice. Press any key...
pause >nul
goto menu

:run_node
echo Starting node...
echo (A new window will open. Close that window to stop the node.)
start "GVR Node" cmd /c "cd /d %~dp0 && gvr_hybrid_node.exe --p2p-addr 0.0.0.0:4000 --rpc-addr 0.0.0.0:8080 --coinbase-addr alice"
echo Node started in separate window.
pause
goto menu

:status
echo Requesting network status...
echo.
client.exe status --rpc 127.0.0.1:8080
echo.
pause
goto menu

:wallet_new
set /p wname=New wallet name: 
if "%wname%"=="" (
  echo Name cannot be empty.
  pause
  goto menu
)
echo Creating wallet "%wname%"...
wallet.exe new --name "%wname%"
echo.
pause
goto menu

:wallet_show
set /p wname=Wallet name: 
if "%wname%"=="" (
  echo Name cannot be empty.
  pause
  goto menu
)
echo Wallet "%wname%" info:
wallet.exe show --name "%wname%"
echo.
pause
goto menu

:wallet_send
set /p fromw=From wallet name: 
if "%fromw%"=="" (
  echo Name cannot be empty.
  pause
  goto menu
)
set /p toaddr=To address (string): 
if "%toaddr%"=="" (
  echo Address cannot be empty.
  pause
  goto menu
)
set /p amount=Amount (integer): 
if "%amount%"=="" (
  echo Amount cannot be empty.
  pause
  goto menu
)
set /p fee=Fee (default 1): 
if "%fee%"=="" set fee=1

echo Sending: from=%fromw% to=%toaddr% amount=%amount% fee=%fee%
wallet.exe send --from-wallet "%fromw%" --to "%toaddr%" --amount %amount% --fee %fee% --rpc 127.0.0.1:8080
echo.
pause
goto menu

:end
echo Bye.