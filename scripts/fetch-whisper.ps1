# Downloads the pinned whisper.cpp CPU build into crates/app/whisper/ (bundled as a resource).
# Run once per checkout, and again after changing $Tag/$Sha256:
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-whisper.ps1
$ErrorActionPreference = 'Stop'

$Tag = 'b5454'   # whisper.cpp v1.9.5
$Asset = 'whisper-bin-x64.zip'
$Sha256 = '6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb'

$Dest = Join-Path $PSScriptRoot '..\crates\app\whisper'
$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) "thera-whisper-$Tag"
New-Item -ItemType Directory -Force $Tmp | Out-Null
$Zip = Join-Path $Tmp $Asset

if (-not (Test-Path $Zip) -or (Get-FileHash $Zip -Algorithm SHA256).Hash -ne $Sha256.ToUpper()) {
    Write-Host "Downloading whisper.cpp $Tag ..."
    Invoke-WebRequest "https://github.com/ggml-org/whisper.cpp/releases/download/$Tag/$Asset" -OutFile $Zip
}
$Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash
if ($Actual -ne $Sha256.ToUpper()) { throw "SHA-256 mismatch for ${Asset}: $Actual" }

$Extract = Join-Path $Tmp 'x'
if (Test-Path $Extract) { Remove-Item -Recurse -Force $Extract }
Expand-Archive $Zip -DestinationPath $Extract

# Only the CLI (recordings), the server (live dictation, keeps the model loaded) and the
# libraries they load (ggml picks the best ggml-cpu-*.dll at runtime).
New-Item -ItemType Directory -Force $Dest | Out-Null
Get-ChildItem $Dest -Exclude '.gitkeep' | Remove-Item -Force
$Wanted = @('whisper-cli.exe', 'whisper-server.exe', 'whisper.dll', 'ggml.dll', 'ggml-base.dll')
Get-ChildItem (Join-Path $Extract 'Release') | Where-Object { $Wanted -contains $_.Name -or $_.Name -like 'ggml-cpu-*.dll' } |
    Copy-Item -Destination $Dest
Set-Content -Path (Join-Path $Dest 'VERSION') -Value $Tag -NoNewline -Encoding ascii
Write-Host "whisper.cpp $Tag -> $((Resolve-Path $Dest).Path)"
Get-ChildItem $Dest | Format-Table Name, Length
