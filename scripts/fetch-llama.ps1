# Downloads the pinned llama.cpp Vulkan build (llama-server) into crates/app/llama/ (bundled as a resource).
# Vulkan runs on NVIDIA, AMD and Intel GPUs; without a usable GPU the same build runs on the CPU.
# Run once per checkout, and again after changing $Tag/$Sha256:
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-llama.ps1
$ErrorActionPreference = 'Stop'

$Tag = 'b11932'
$Asset = "llama-$Tag-bin-win-vulkan-x64.zip"
$Sha256 = '64e4872836d633f496a46504f6fea407d1fdd4e84f6137df33cae513e3fbd13c'

$Dest = Join-Path $PSScriptRoot '..\crates\app\llama'
$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) "thera-llama-$Tag"
New-Item -ItemType Directory -Force $Tmp | Out-Null
$Zip = Join-Path $Tmp $Asset

if (-not (Test-Path $Zip) -or (Get-FileHash $Zip -Algorithm SHA256).Hash -ne $Sha256.ToUpper()) {
    Write-Host "Downloading llama.cpp $Tag ..."
    Invoke-WebRequest "https://github.com/ggml-org/llama.cpp/releases/download/$Tag/$Asset" -OutFile $Zip
}
$Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash
if ($Actual -ne $Sha256.ToUpper()) { throw "SHA-256 mismatch for ${Asset}: $Actual" }

$Extract = Join-Path $Tmp 'x'
if (Test-Path $Extract) { Remove-Item -Recurse -Force $Extract }
Expand-Archive $Zip -DestinationPath $Extract

# Only llama-server and the libraries it loads. ggml loads every ggml-*.dll next to it as a
# backend, so ggml-rpc.dll stays out; it picks the best ggml-cpu-*.dll for the CPU at runtime.
New-Item -ItemType Directory -Force $Dest | Out-Null
Get-ChildItem $Dest -Exclude '.gitkeep' | Remove-Item -Force
$Wanted = @('llama-server.exe', 'llama-server-impl.dll', 'llama-common.dll', 'llama.dll', 'mtmd.dll',
    'ggml.dll', 'ggml-base.dll', 'ggml-vulkan.dll', 'libomp.dll', 'LICENSE-LLVM-OpenMP')
Get-ChildItem $Extract | Where-Object { $Wanted -contains $_.Name -or $_.Name -like 'ggml-cpu-*.dll' } |
    Copy-Item -Destination $Dest
Set-Content -Path (Join-Path $Dest 'VERSION') -Value $Tag -NoNewline -Encoding ascii
Write-Host "llama.cpp $Tag -> $((Resolve-Path $Dest).Path)"
Get-ChildItem $Dest | Format-Table Name, Length
