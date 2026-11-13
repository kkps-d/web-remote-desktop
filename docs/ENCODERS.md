# Encoder picks

Candidate H.264 encoders for each kind of hardware, to verify on real machines. Only H.264 is picked because almost every viewer, including smart TVs and low-power devices, decodes it in hardware.

Each group has a main pick and a fallback that loads through a different driver path, so one failing doesn't take out the other.

## Picks

| Hardware | Main | Fallback |
|---|---|---|
| NVIDIA (Pascal+) | `nvd3d11h264enc tune=ultra-low-latency bframes=0 rc-lookahead=0` | `mfh264enc low-latency=true` (NVIDIA's Media Foundation encoder) |
| AMD (Polaris+) | `amfh264enc usage=ultra-low-latency` | `mfh264enc low-latency=true` (AMD's Media Foundation encoder) |
| Generic GPU (DX11/DX12) | `mfh264enc low-latency=true` | `d3d12h264enc` (D3D12 capture) |
| CPU | `x264enc tune=zerolatency speed-preset=ultrafast` | Microsoft's software Media Foundation encoder ("H264 Encoder MFT") with `low-latency=true` |

Notes:

- `x264enc` is GPL. If that blocks distribution, `openh264enc` (BSD) is the alternative, but it was about 10× slower here.
- `d3d12h264enc` needs a recent GPU and driver with D3D12 Video encode support.
- AMF has only been tested on a Radeon iGPU, where it took ~17 ms per frame. A discrete card should be faster; this is the main thing to confirm.
- AV1 would give better quality per bit but needs NVIDIA Ada (RTX 40) or AMD RDNA3 or newer, and many TVs can't decode it in hardware.

## Element names

Names vary per machine. GStreamer registers one element per GPU, and extra adapters get a `deviceN` suffix (`mfh264device1enc`, `d3d12h264device2enc`). The unsuffixed name is the first adapter. List them with their vendor or GPU name:

```powershell
& "$env:LOCALAPPDATA\Programs\gstreamer\1.0\msvc_x86_64\bin\gst-inspect-1.0.exe" | Select-String "h264.*enc"
```

The Microsoft software encoder is the `mfh264*enc` whose description says "H264 Encoder MFT" without a vendor name.

## Testing

[`scripts/test-encoder.ps1`](../scripts/test-encoder.ps1) captures the screen like the agent does and reports per-frame encode time. Pass the encoder with its properties; add `-Cpu` for CPU encoders and `-D3D12` for `d3d12*enc` encoders, which can't take D3D11 frames.

```powershell
.\scripts\test-encoder.ps1 "nvd3d11h264enc tune=ultra-low-latency bframes=0 rc-lookahead=0"
.\scripts\test-encoder.ps1 "x264enc tune=zerolatency speed-preset=ultrafast" -Cpu
.\scripts\test-encoder.ps1 "mfh264device2enc low-latency=true" -Cpu
.\scripts\test-encoder.ps1 "d3d12h264enc" -D3D12
```

- Play a video or scroll a page during the run; a static screen is trivially cheap to encode.
- Defaults are 1080p60 and 600 frames. Change with `-Width`, `-Height`, `-Fps`, `-Frames`.
- A pick passes if p95 stays well under the frame time (16.7 ms at 60 fps) and overall fps stays near the target.
- A warning that only some frames were timed means the encoder dropped or reordered frames (B-frames). Its numbers are unreliable, so turn off B-frames and lookahead.
- If execution policy blocks the script: `powershell -ExecutionPolicy Bypass -File .\scripts\test-encoder.ps1 ...`

Viewer-side decoding (smart TVs) can only be checked through the agent, which currently uses `mfh264enc`. To try another encoder there, change the encoder in `PipelineDescription` in `agent/StreamSession.cs`, keeping `name=encoder` and `bitrate`, and replacing `gop-size` with the encoder's own keyframe interval property (`key-int-max` for `x264enc`). CPU encoders need `d3d11download !` between the `output` capsfilter and the encoder, because they only accept system memory. `d3d12*enc` also needs the D3D12 capture elements there.

## Results

Baseline machine: RTX 5080, Ryzen 7 7800X3D with Radeon iGPU, GStreamer 1.28.8. Measured at 1080p60 on a synthetic test pattern; CPU numbers are optimistic.

| Machine | Encoder | Works | Mean | p95 | Notes |
|---|---|---|---|---|---|
| RTX 5080 | `nvd3d11h264enc` | ✅ | 2.2 ms | 2.9 ms | |
| RTX 5080 | `mfh264enc` | ✅ | 2.5 ms | 3.1 ms | |
| RTX 5080 | `d3d12h264enc` | ✅ | 2.4 ms | 2.9 ms | |
| Radeon iGPU (Raphael) | `amfh264enc` | ✅ | 17 ms | 18 ms | About one frame of delay |
| Ryzen 7 7800X3D | `x264enc` | ✅ | 1.2 ms | 1.4 ms | 1.5 ms on noise |
| Ryzen 7 7800X3D | Microsoft MFT | ✅ | 1.7 ms | 1.8 ms | |
