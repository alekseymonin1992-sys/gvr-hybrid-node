@echo off
REM ???????????? GVR Hybrid Node
REM P2P: 0.0.0.0:4000, RPC: 0.0.0.0:8080, coinbase=alice

cd /d %~dp0

echo Starting GVR Hybrid Node...
echo P2P: 0.0.0.0:4000
echo RPC: 0.0.0.0:8080
echo Coinbase address: alice
echo.

gvr_hybrid_node.exe --p2p-addr 0.0.0.0:4000 --rpc-addr 0.0.0.0:8080 --coinbase-addr alice

pause
