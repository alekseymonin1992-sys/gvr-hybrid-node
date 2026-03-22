param(
    [string]$Configuration = "release"
)

Write-Host "=== Building binaries (cargo build --$Configuration) ==="

# Сборка
cargo build --$Configuration
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build failed"
    exit 1
}

# Папка проекта (текущая)
$root = Get-Location

# Папка dist
$dist = Join-Path $root "dist"

Write-Host "=== Creating dist directory: $dist ==="
New-Item -ItemType Directory -Path $dist -Force | Out-Null

# Копируем бинарники
$binaries = @("gvr_hybrid_node.exe", "client.exe", "wallet.exe")
foreach ($bin in $binaries) {
    $src = Join-Path $root "target\$Configuration\$bin"
    $dst = Join-Path $dist $bin
    if (-Not (Test-Path $src)) {
        Write-Error "Binary not found: $src"
        exit 1
    }
    Write-Host "Copying $src -> $dst"
    Copy-Item $src $dst -Force
}

# Функция записи bat-файла
function Write-BatFile($name, $content) {
    $path = Join-Path $dist $name
    Write-Host "Creating $path"
    $content | Out-File -FilePath $path -Encoding ASCII -Force
}

# run_node.bat
$runNode = @"
@echo off
REM Запуск GVR Hybrid Node
REM P2P: 0.0.0.0:4000, RPC: 0.0.0.0:8080, coinbase=alice

cd /d %~dp0

echo Starting GVR Hybrid Node...
echo P2P: 0.0.0.0:4000
echo RPC: 0.0.0.0:8080
echo Coinbase address: alice
echo.

gvr_hybrid_node.exe --p2p-addr 0.0.0.0:4000 --rpc-addr 0.0.0.0:8080 --coinbase-addr alice

pause
"@
Write-BatFile "run_node.bat" $runNode

# run_client_status.bat
$runStatus = @"
@echo off
cd /d %~dp0

echo GVR Client: status
echo.

client.exe status --rpc 127.0.0.1:8080

pause
"@
Write-BatFile "run_client_status.bat" $runStatus

# run_wallet_new_alice.bat
$runWalletNew = @"
@echo off
cd /d %~dp0

echo GVR Wallet: create wallet 'alice'
echo.

wallet.exe new --name alice

pause
"@
Write-BatFile "run_wallet_new_alice.bat" $runWalletNew

# run_wallet_send_example.bat
$runWalletSend = @"
@echo off
cd /d %~dp0

echo GVR Wallet: send from alice to bob (amount=10, fee=1)
echo.

wallet.exe send --from-wallet alice --to bob --amount 10 --fee 1 --rpc 127.0.0.1:8080

pause
"@
Write-BatFile "run_wallet_send_example.bat" $runWalletSend

Write-Host "=== dist setup complete ==="
Write-Host "dist directory: $dist"