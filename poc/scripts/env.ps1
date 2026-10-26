# Dot-source before building or testing:  . .\poc\scripts\env.ps1
# Locates the toolchain and the LGPL FFmpeg dev package installed with:
#   winget install Rustlang.Rustup LLVM.LLVM BtbN.FFmpeg.LGPL.Shared.8.1 bluenviron.mediamtx
$ErrorActionPreference = 'Stop'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$pkgs = "$env:LOCALAPPDATA\Microsoft\WinGet\Packages"
$ffRoot = Get-ChildItem $pkgs -Directory | Where-Object Name -like 'BtbN.FFmpeg.LGPL.Shared.8.1*' | Select-Object -First 1
if (-not $ffRoot) { throw 'FFmpeg LGPL shared 8.1 not found: winget install BtbN.FFmpeg.LGPL.Shared.8.1' }
$env:FFMPEG_DIR = (Get-ChildItem $ffRoot.FullName -Directory | Select-Object -First 1).FullName
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
# FFmpeg DLLs first on PATH so the engine loads the LGPL build, not another ffmpeg on the machine.
$env:Path = "$env:FFMPEG_DIR\bin;$env:Path"
$mtx = Get-ChildItem $pkgs -Directory | Where-Object Name -like 'bluenviron.mediamtx*' | Select-Object -First 1
if ($mtx) { $env:JK_MEDIAMTX = Join-Path $mtx.FullName 'mediamtx.exe' }
Write-Host "FFMPEG_DIR=$env:FFMPEG_DIR"
Write-Host "JK_MEDIAMTX=$env:JK_MEDIAMTX"
