# context-snipe installer — Windows
#
#   irm https://raw.githubusercontent.com/RP-Digital-Innovations/context-snipe/main/install.ps1 | iex
#
# Downloads the prebuilt Windows binary from the latest GitHub release and
# installs it to %LOCALAPPDATA%\context-snipe, adding that dir to your user PATH.
$ErrorActionPreference = 'Stop'

$Repo  = 'RP-Digital-Innovations/context-snipe'
$Asset = 'context-snipe-x86_64-pc-windows.exe'
$Url   = "https://github.com/$Repo/releases/latest/download/$Asset"

$InstallDir = Join-Path $env:LOCALAPPDATA 'context-snipe'
$Dest       = Join-Path $InstallDir 'context-snipe.exe'

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

Write-Host "Downloading $Asset..."
Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing

Write-Host ""
Write-Host "  Installed context-snipe -> $Dest"

# Add to the user PATH if it isn't already there.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$InstallDir*") {
    $newPath = if ([string]::IsNullOrEmpty($userPath)) { $InstallDir } else { "$userPath;$InstallDir" }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "  Added $InstallDir to your user PATH (restart terminals to pick it up)."
}

Write-Host ""
Write-Host "  Verify:   context-snipe --version"
Write-Host "  Next:     add it to your AI tool - https://github.com/$Repo#add-to-your-ai-tool-60-seconds"
Write-Host ""
