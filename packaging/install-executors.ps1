# Instala los ejecutores de este paquete donde un `runtime` lógico se resuelve
# (ADR-0046 §4): `%USERPROFILE%\.anvil\executors`, o `$env:ANVIL_HOME\executors`.
#
# Se copia y ya. No hay registro, ni servicio, ni nada que arrancar: un
# ejecutor se instala una vez y quien lo levanta es el arranque de la máquina,
# un gestor de servicios, o una persona — nunca el motor, y nunca una secuencia.
#
#   .\executors\install.ps1
#   $env:ANVIL_HOME = "C:\anvil"; .\executors\install.ps1

$ErrorActionPreference = "Stop"

$Aqui = $PSScriptRoot
$Base = if ($env:ANVIL_HOME) { $env:ANVIL_HOME } else { Join-Path $env:USERPROFILE ".anvil" }
$Destino = Join-Path $Base "executors"

New-Item -ItemType Directory -Force -Path $Destino | Out-Null
foreach ($runtime in Get-ChildItem -Path $Aqui -Directory) {
    if (-not (Test-Path (Join-Path $runtime.FullName "executor.json"))) { continue }
    $target = Join-Path $Destino $runtime.Name
    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Copy-Item -Recurse -Force (Join-Path $runtime.FullName "*") $target
    Write-Host "  $($runtime.Name) -> $target"
}

Write-Host "ejecutores instalados en $Destino"
