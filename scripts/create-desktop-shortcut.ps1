# Creates a "thera" shortcut on the desktop pointing to the release build.
# Build first: cd ui; npm run app:build  (or app:build:plain without Strawberry Perl)
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "target\release\thera-app.exe"
if (-not (Test-Path $exe)) { throw "Not built yet: $exe" }

$desktop = [Environment]::GetFolderPath("Desktop")
$shell = New-Object -ComObject WScript.Shell
$lnk = $shell.CreateShortcut((Join-Path $desktop "thera.lnk"))
$lnk.TargetPath = $exe
$lnk.WorkingDirectory = Split-Path $exe
$lnk.IconLocation = "$exe,0"
$lnk.Description = "thera - Dokumentation"
$lnk.Save()
Write-Output "Shortcut created: $(Join-Path $desktop 'thera.lnk')"
