# Downloads the pinned sherpa-onnx CPU runtime and the two speaker models into crates/app/diarize/
# (bundled as a resource; speaker separation of session recordings, slice 27).
# Run once per checkout, and again after changing a version or hash:
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-sherpa.ps1
# The struct layouts in crates/diarize/src/lib.rs follow this version's c-api.h: check them on an upgrade.
$ErrorActionPreference = 'Stop'

$Tag = 'v2.0.1'
$Files = @(
    @{ Name = "sherpa-onnx-$Tag-win-x64-shared-MD-Release-no-tts.tar.bz2"
       Url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/$Tag/sherpa-onnx-$Tag-win-x64-shared-MD-Release-no-tts.tar.bz2"
       Sha256 = '11fd6a50b97eed38b54fb16d8219f49b16aa7f6a8e43a042316469ef47652e82' },
    # pyannote segmentation 3.0 (MIT), who speaks when
    @{ Name = 'sherpa-onnx-pyannote-segmentation-3-0.tar.bz2'
       Url = 'https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2'
       Sha256 = '24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488' },
    # 3D-Speaker ERes2Net (VoxCeleb; Apache-2.0), what a voice sounds like. Chosen over WeSpeaker
    # ResNet34, which merged two speakers on German test audio and on one of sherpa's English samples.
    @{ Name = '3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx'
       Url = 'https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx'
       Sha256 = 'c59158379255ad66e161679cca6af8d52d51e389e3224ab7d7a7baae295c2db5' }
)

$Dest = Join-Path $PSScriptRoot '..\crates\app\diarize'
$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) "thera-sherpa-$Tag"
New-Item -ItemType Directory -Force $Tmp | Out-Null

foreach ($f in $Files) {
    $Path = Join-Path $Tmp $f.Name
    if (-not (Test-Path $Path) -or (Get-FileHash $Path -Algorithm SHA256).Hash -ne $f.Sha256.ToUpper()) {
        Write-Host "Downloading $($f.Name) ..."
        Invoke-WebRequest $f.Url -OutFile $Path
    }
    $Actual = (Get-FileHash $Path -Algorithm SHA256).Hash
    if ($Actual -ne $f.Sha256.ToUpper()) { throw "SHA-256 mismatch for $($f.Name): $Actual" }
}

$Extract = Join-Path $Tmp 'x'
if (Test-Path $Extract) { Remove-Item -Recurse -Force $Extract }
New-Item -ItemType Directory -Force $Extract | Out-Null
tar -xjf (Join-Path $Tmp $Files[0].Name) -C $Extract
tar -xjf (Join-Path $Tmp $Files[1].Name) -C $Extract
if ($LASTEXITCODE -ne 0) { throw 'tar failed' }

# Only the C API library and the onnxruntime it loads, plus the models under fixed names.
New-Item -ItemType Directory -Force $Dest | Out-Null
Get-ChildItem $Dest -Exclude '.gitkeep' | Remove-Item -Force
$Lib = Join-Path $Extract "sherpa-onnx-$Tag-win-x64-shared-MD-Release-no-tts\lib"
foreach ($n in @('sherpa-onnx-c-api.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll')) {
    Copy-Item (Join-Path $Lib $n) -Destination $Dest
}
Copy-Item (Join-Path $Extract 'sherpa-onnx-pyannote-segmentation-3-0\model.onnx') -Destination (Join-Path $Dest 'segmentation.onnx')
Copy-Item (Join-Path $Tmp $Files[2].Name) -Destination (Join-Path $Dest 'embedding.onnx')
Set-Content -Path (Join-Path $Dest 'VERSION') -Value "sherpa-onnx-$Tag+pyannote-3.0+3dspeaker-eres2net-voxceleb" -NoNewline -Encoding ascii
Write-Host "sherpa-onnx $Tag -> $((Resolve-Path $Dest).Path)"
Get-ChildItem $Dest | Format-Table Name, Length
