@echo off
cd /d %~dp0

REM Сборка (если нужно) - можно закомментировать, если уже собрано
REM cargo build --release

target\release\gvr-node.exe ^
  --p2p-addr 0.0.0.0:4000 ^
  --rpc-addr 127.0.0.1:8080 ^
  --coinbase-addr alekseymonin1992 ^
  --ai-key-file ai_key.bin

pause