# PowerShell script to build and run GVR v2
chcp 65001 | Out-Null

Write-Host "Compiling Rust project..."
cargo build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Running GVR v2..."
cargo run
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Test complete. Blocks and rewards displayed above."