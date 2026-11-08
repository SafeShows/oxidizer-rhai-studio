# Builds the mod and copies it into the game's mods folder (Windows). Usage: ./install.ps1 [mods folder]
# Quit the game first: Windows keeps the loaded library locked.
param([string]$Mods = (Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'My Games\Kitten Space Agency\mods'))
$ErrorActionPreference = 'Stop'
$id = (Select-String -Path Cargo.toml -Pattern '^name = "([^"]+)"').Matches[0].Groups[1].Value
cargo build --release
if ($LASTEXITCODE) { exit 1 }
$dest = Join-Path $Mods $id
New-Item -ItemType Directory -Force (Join-Path $dest 'bin') | Out-Null
Copy-Item mod\* $dest -Recurse -Force
Copy-Item "target\release\$id.dll" (Join-Path $dest 'bin') -Force
"Installed to $dest"
