# Known issues

Each issue is tagged with the platform it affects.

## Streaming

- **Framerate can exceed the selected rate** (Windows). At 120 fps, the Android overlay reached 144 fps and the agent averaged 132 encoded fps over 12 seconds. The agent sets the requested rate in the capture caps and relies on the capture source for pacing. A fresh session held near 120 fps, so the trigger is still unknown.
- **NVENC bitrate can exceed the selected rate** (Windows). At 8 Mbps, a fullscreen motion test peaked at 14.9 Mbps in the overlay. The setting is an average target for variable bitrate encoding, with the peak left to NVENC. The overlay shows the receive rate over roughly one second.
- **Named PipeWire capture needs a configured audio sink** (Linux). Set `pipewire_audio_sink` to stream its monitor. Gamescope keyboard and mouse input requires the EIS socket. The viewer can use touch to emulate a mouse.
- **The stream cursor can't be hidden** (Linux). The ScreenCast portal fixes the cursor mode per session, so the cursor is always embedded.
- **One monitor only** (Linux). Absolute and touch coordinates cover the whole desktop, which matches the stream only with one monitor.
- **Named PipeWire capture uses x264 on multi-GPU Linux systems.** Gamescope does not report its render GPU through PipeWire. Portal capture uses the GNOME compositor's render GPU when it is available.

## Audio

- **Viewing on the agent's own computer echoes when unmuted** (all). The agent captures everything the computer plays, including the client's playback. Sessions with an agent on `localhost` start muted; a LAN address of the same computer doesn't. Linux has no equivalent of Windows' process-loopback exclusion, so the agent's own output is captured too.
- **No audio on Windows builds before 20348** (Windows). Process loopback needs Windows 10 build 20348 or later (e.g. Windows 11); older builds stream video only.
- **Process loopback adds about 40 ms** (Windows). The spike measured 135–138 ms against 96 ms for loopback of the default output device, which stops for good when the output device switches. See the [audio spike](spikes/audio.md); the [roadmap](ROADMAP.md#v115-audio-capture-modes-optional) lists the alternatives.
- **The host's volume and mute don't apply** (all). Capture happens before them, so the viewer hears full volume while the host is muted, and the host's speakers keep playing.

## Encoders

- **Media Foundation can break NVENC until the agent restarts** (Windows). After NVENC has run, using a Media Foundation encoder in the same process makes later NVENC sessions fail with `NV_ENC_ERR_INVALID_VERSION`. Reproduced with GStreamer 1.28.8 and NVIDIA driver 610.88. The agent disables the Media Foundation encoders when NVENC is available.
- **AMF was slow on the tested Radeon iGPU** (Windows). About 17 ms per frame at 1080p60, close to the full frame budget. A discrete Radeon card is unverified. See the [encoder benchmarks](ENCODERS.md#results).

## Input

- **Elevated windows ignore input** (Windows). Windows blocks injected input to elevated windows unless the agent also runs elevated (UIPI).
- **The secure desktop is out of reach** (Windows). UAC prompts and the lock screen run on the secure desktop, which the agent can neither capture nor control.
- **Canceled touches end like lifts** (all). Windows ignores the cancel flag on synthetic touch, and Linux has no touch cancel, so a touch the agent cancels (on `releaseAll` or a disconnect) can still tap or click.
- **The agent needs Windows 10 1809 or later** (Windows). Older versions lack the synthetic pointer devices it uses for touch, so it doesn't start there.
- **Ctrl+Alt+Del can't be injected** (Windows). `SendInput` can't produce it. `SendSAS` can, but only from a service or with the "software SAS" policy enabled.
- **Input needs the `input` group** (Linux). Access to `/dev/uinput` comes from the `input` group, which can also read every input device. Fine on a dev VM, not something to ship. Without it the agent streams without input.
- **Relative moves are accelerated** (Linux). The desktop applies its pointer acceleration to the relative mouse device, so relative mode isn't 1:1 as on Windows.
- **Esc exits relative mode** (all). Browsers end Pointer Lock on Esc, so Esc reaches the remote computer only while Keyboard Lock is active (`localhost` in fullscreen).

## Plain HTTP

- **HTTP limits signaling and browser APIs** (all). Network peers can tamper with unencrypted signaling. Browsers block requests from an HTTPS client page to a remote HTTP agent. Secure-context APIs such as clipboard and Keyboard Lock require a trustworthy origin, such as `localhost` during HTTP development.

## GStreamer workarounds

- **The agent needs GStreamer on `PATH`** (Windows). The per-user installer leaves it off. Windows loads GStreamer's DLLs before the agent starts, so it can't add the directory itself. Without it the agent doesn't start (exit code `0xC0000135`, a missing DLL). Building needs `PKG_CONFIG` and `PKG_CONFIG_PATH` too; see the [README](../README.md#windows).
- **`webrtcbin` matches the offer against exact RTP caps** (all). The agent waits for sink caps, uses the browser's H.264 payload type, and matches its `profile-level-id` via `capssetter`. This lets the stream use the higher H.264 level needed for desktop resolutions when the browser allows level asymmetry.
