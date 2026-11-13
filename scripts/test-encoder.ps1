# Measures how long one GStreamer encoder takes per frame on live screen capture.
# See docs/ENCODERS.md for the encoders to test.
#
#   .\scripts\test-encoder.ps1 "nvd3d11h264enc tune=ultra-low-latency bframes=0 rc-lookahead=0"
#   .\scripts\test-encoder.ps1 "x264enc tune=zerolatency speed-preset=ultrafast" -Cpu
#   .\scripts\test-encoder.ps1 "d3d12h264enc" -D3D12
param(
    [Parameter(Mandatory)] [string] $Encoder,  # element name plus properties
    [switch] $Cpu,                             # copy frames to system memory, as CPU encoders need
    [switch] $D3D12,                           # capture with D3D12, as d3d12*enc encoders need
    [int] $Width = 1920,
    [int] $Height = 1080,
    [int] $Fps = 60,
    [int] $Frames = 600
)

$root = $env:GSTREAMER_1_0_ROOT_MSVC_X86_64
if (-not $root) { $root = "$env:LOCALAPPDATA\Programs\gstreamer\1.0\msvc_x86_64" }
$launch = Join-Path $root "bin\gst-launch-1.0.exe"

# Same capture path as the agent: the capture source runs at the frame rate in the caps.
$caps = "width=$Width,height=$Height,framerate=$Fps/1"
$capture = if ($Cpu) {
    "d3d11screencapturesrc num-buffers=$Frames ! d3d11convert ! video/x-raw(memory:D3D11Memory),$caps ! d3d11download ! videoconvert ! video/x-raw,format=I420"
} elseif ($D3D12) {
    "d3d12screencapturesrc num-buffers=$Frames ! d3d12convert ! video/x-raw(memory:D3D12Memory),format=NV12,$caps"
} else {
    "d3d11screencapturesrc num-buffers=$Frames ! d3d11convert ! video/x-raw(memory:D3D11Memory),format=NV12,$caps"
}
$pipeline = "$capture ! $Encoder name=enc ! fakesink"

# The latency tracer logs how long each frame spent inside each element.
$env:GST_TRACERS = "latency(flags=element)"
$env:GST_DEBUG = "GST_TRACER:7"
$env:GST_DEBUG_NO_COLOR = "1"
$started = Get-Date
$log = & $launch -q ($pipeline -split " ") 2>&1 | ForEach-Object { "$_" }
$exitCode = $LASTEXITCODE
$seconds = ((Get-Date) - $started).TotalSeconds
Remove-Item Env:GST_TRACERS, Env:GST_DEBUG, Env:GST_DEBUG_NO_COLOR

if ($exitCode -ne 0) {
    Write-Host "FAILED (exit code $exitCode)" -ForegroundColor Red
    $log | Where-Object { $_ -notmatch "GST_TRACER" } | Select-Object -Last 10
    exit 1
}

$times = @($log |
    Where-Object { $_ -match "element=\(string\)enc," } |
    ForEach-Object { [double]($_ -replace ".*time=\(guint64\)(\d+).*", '$1') / 1e6 } |
    Sort-Object)
if ($times.Count -eq 0) {
    Write-Host "No timing samples; the encoder produced no output." -ForegroundColor Red
    exit 1
}

$mean = ($times | Measure-Object -Average).Average
$p95 = $times[[int][math]::Floor($times.Count * 0.95) - 1]
"{0}`n  {1}x{2}@{3}  mean {4:N2} ms  p95 {5:N2} ms  max {6:N2} ms  overall {7:N0} fps (incl. startup)" -f `
    $Encoder, $Width, $Height, $Fps, $mean, $p95, $times[-1], ($Frames / $seconds)
