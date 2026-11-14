# Known issues

## Streaming

- **Framerate can exceed the selected rate.** At 120 fps, the Android overlay reached 144 fps and the agent averaged 132 encoded fps over 12 seconds. The agent sets the requested rate in the GStreamer capture caps and relies on the capture source for pacing. A fresh session held near 120 fps, so the trigger in the affected session is still unknown.
- **NVENC bitrate can exceed the selected rate.** At 8 Mbps, a fullscreen motion test peaked at 14.9 Mbps in the overlay. The setting is an average target for variable bitrate encoding, with the peak left to NVENC. The overlay shows the receive rate over roughly one second.

## Encoders

- **Media Foundation can break NVENC until the agent restarts.** After NVENC has run, using a Media Foundation encoder in the same process makes later NVENC sessions fail with `NV_ENC_ERR_INVALID_VERSION`. Reproduced with GStreamer 1.28.8 and NVIDIA driver 610.88. The agent disables the hardware and software Media Foundation encoders when NVENC is available.
- **AMF was slow on the tested Radeon iGPU.** It took about 17 ms per frame at 1080p60, close to the full frame budget. Performance on a discrete Radeon card remains unverified. See the [encoder benchmarks](ENCODERS.md#results).

## Input

- **Elevated windows ignore input.** Windows blocks injected input to elevated windows unless the agent also runs elevated (UIPI).
- **The secure desktop is out of reach.** UAC prompts and the lock screen run on the secure desktop, which the agent can neither capture nor control.
- **Ctrl+Alt+Del can't be injected.** `SendInput` can't produce it. `SendSAS` can, but only from a service or with the "software SAS" policy enabled.

## Plain HTTP

- **HTTP limits signaling and browser APIs.** Network peers can tamper with unencrypted signaling. Browsers block requests from an HTTPS client page to a remote HTTP agent. Secure-context APIs such as clipboard and Keyboard Lock require a trustworthy origin, such as `localhost` during HTTP development.

## GStreamer workarounds

- **The per-user Windows installer leaves GStreamer off `PATH`.** The agent adds the GStreamer `bin` directory at startup.
- **GLibSharp expects MinGW DLL names.** The agent preloads the MSVC DLL handles in the binding's cache.
- **The C# bindings have signal and SDP handling limitations.** Signal calls that need a null argument use an empty `Structure`. The agent reads `local-description` via `g_object_get` to avoid premature SDP disposal.
- **`webrtcbin` matches the offer against exact RTP caps.** The agent waits for sink caps, uses the browser's H.264 payload type, and matches its `profile-level-id` via `capssetter`. This lets the stream use the higher H.264 level needed for desktop resolutions when the browser allows level asymmetry.
