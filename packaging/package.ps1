# Windows sibling of package.sh: builds the downloadable package for the
# engine — `anvil.exe` and `anvil-exec-wasm.exe` — plus its SHA256SUMS.
#
# This is the engine's own package, independent of the Sequence Editor's
# installer (an Electron app, built separately by `npm run app:build` in
# `editor/`, ADR-0037). Someone running headless (CI, a bench, scripting)
# wants this one; someone developing sequences wants the editor installer
# instead.
#
# Usage (from the repo root, PowerShell):
#   .\packaging\package.ps1            # version taken from the host's manifest
#   .\packaging\package.ps1 0.4.0      # or given explicitly
#
# Needs the MSVC target (`rustup target add x86_64-pc-windows-msvc`) and the
# WASM guests, built here in the order ADR-0011/ADR-0015 require. Compiled
# natively — no cross-compilation — so it only runs on Windows.

param(
    [string]$Version
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$Target = "x86_64-pc-windows-msvc"
$Wasm = "wasm32-wasip2"
if (-not $Version) {
    $Version = (Select-String -Path packaging/anvil-host/Cargo.toml -Pattern '^version' | Select-Object -First 1).Line -replace '.*"(.*)".*', '$1'
}
$PkgName = "anvil-v$Version-x86_64-windows"
$Out = "dist"

Write-Host "== building $PkgName =="

# Static CRT so the result needs no Visual C++ redistributable installed —
# the same "download and run" promise the Linux musl build makes.
$env:RUSTFLAGS = "-C target-feature=+crt-static"

# The guest carries the version `anvil --version` prints, so they are built
# first and from the current manifest — not reused from a previous bump.
cargo build --release --target $Wasm -p motor
cargo build --release --target $Wasm --manifest-path ejemplos/hola-paso/Cargo.toml
cargo build --release --target $Wasm --manifest-path ejemplos/departamento/Cargo.toml
cargo build --release --target $Target --manifest-path executors/wasm/Cargo.toml
cargo build --release --target $Target --manifest-path packaging/anvil-host/Cargo.toml

$PkgDir = Join-Path $Out $PkgName
if (Test-Path $PkgDir) { Remove-Item -Recurse -Force $PkgDir }
New-Item -ItemType Directory -Force -Path (Join-Path $PkgDir "ejemplos/departamento/dist") | Out-Null

Copy-Item "packaging/anvil-host/target/$Target/release/anvil.exe" $PkgDir
Copy-Item "executors/wasm/target/$Target/release/anvil-exec-wasm.exe" $PkgDir
Copy-Item README.md, CHANGELOG.md $PkgDir
Copy-Item LICENSE (Join-Path $PkgDir "LICENSE")                       # anvil: AGPL-3.0-or-later
Copy-Item executors/LICENSE (Join-Path $PkgDir "LICENSE.executors")   # anvil-exec-wasm: Apache-2.0
Copy-Item ejemplos/*.yseq, ejemplos/*.yaml (Join-Path $PkgDir "ejemplos")

# The example department: the executor's binary with its modules beside it,
# which is what the demos' `path:` points at (ADR-0027).
$DeptDir = Join-Path $PkgDir "ejemplos/departamento/dist"
Copy-Item "executors/wasm/target/$Target/release/anvil-exec-wasm.exe" $DeptDir
Copy-Item "ejemplos/departamento/target/$Wasm/release/*.wasm" $DeptDir
Copy-Item "ejemplos/hola-paso/target/$Wasm/release/*.wasm" $DeptDir

$ZipPath = Join-Path $Out "$PkgName.zip"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path $PkgDir -DestinationPath $ZipPath

$Hash = Get-FileHash -Algorithm SHA256 $ZipPath
"$($Hash.Hash.ToLower())  $PkgName.zip" | Out-File -Encoding ascii (Join-Path $Out "SHA256SUMS")

Write-Host ""
Write-Host "package -> $ZipPath"
Get-Content (Join-Path $Out "SHA256SUMS")
