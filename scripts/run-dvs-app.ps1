# Launch dvs-app with the local WinGet FFmpeg layout.
# Usage:
#   .\scripts\run-dvs-app.ps1
#   .\scripts\run-dvs-app.ps1 -DiagnoseResize
#   .\scripts\run-dvs-app.ps1 -VideoPath docs/fixtures/test_4k_hevc_8bit30.mp4

param(
    [string]$VideoPath = "docs/fixtures/test_4k_hevc_8bit30.mp4",
    [switch]$DiagnoseResize
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

$ffCandidates = @(
    [Environment]::GetEnvironmentVariable("FFMPEG_DIR", "User"),
    [Environment]::GetEnvironmentVariable("FFMPEG_DIR", "Machine"),
    $env:FFMPEG_DIR,
    "C:\Users\PC-STUDIO\AppData\Local\Microsoft\WinGet\Packages\Gyan.FFmpeg.Shared_Microsoft.Winget.Source_8wekyb3d8bbwe\ffmpeg-9.0.1-full_build-shared",
    (Join-Path (Get-Location) "third_party\ffmpeg")
) | Where-Object { $_ -and $_.Trim() -ne "" }

$ff = $null
foreach ($candidate in $ffCandidates) {
    if ((Test-Path (Join-Path $candidate "lib\avcodec.lib")) -and
        (Test-Path (Join-Path $candidate "include\libavcodec\avcodec.h"))) {
        $ff = $candidate
        break
    }
}

if (-not $ff) {
    Write-Error "FFmpeg not found. Set FFMPEG_DIR to a shared MSVC build with include/ and lib/."
}

$env:FFMPEG_DIR = $ff
$env:PATH = "$ff\bin;$env:PATH"
if (-not $env:LIBCLANG_PATH) {
    if (Test-Path "C:\Program Files\LLVM\bin") {
        $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
    }
}

Write-Host "FFMPEG_DIR=$env:FFMPEG_DIR"
Write-Host "VideoPath=$VideoPath"

$cargoArgs = @("-p", "dvs-app", "--release", "--", "--input", $VideoPath)
if ($DiagnoseResize) {
    $cargoArgs += "--diagnose-resize"
}
cargo run @cargoArgs
