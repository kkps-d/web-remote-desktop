# Audio spike

Branch `spike/audio`, Windows. Checked WASAPI loopback into `webrtcbin` next to the video. Tested on localhost with the client muted, so the loopback didn't capture its own playback, and from a second device on the LAN.

The pipeline branch is the one in [Architecture](../ARCHITECTURE.md#windows-capture).

## Results

- **Works first try:** the answer has `OPUS/48000/2` as `sendonly`, and both tracks share one `msid` stream, so Chrome lip-syncs them. Lip sync looked right from the second device.
- **Silence:** both loopback modes keep producing samples when nothing plays. A steady 100 packets/s, 129 kbps, no concealment.
- **Output device switch:**
  - Default-device loopback (no `loopback-mode`): the stream froze, video included, with no pipeline error. With `provide-clock=false` the video kept going, but audio stopped at the switch and never came back.
  - Process loopback: audio packets never dropped across switching, muting and switching back (15 min). A beep afterwards came through at full level.
- **Level:** process loopback captures before the Windows volume and mute (a 0.15 beep arrived at 0.155). Default-device loopback captured after the volume (0.03).
- **Latency**, from a beep on the agent machine to its arrival in the browser:
  - 96 ms with default-device loopback, 135–138 ms with process loopback.
  - Peaked at 172 ms right after a device switch, then settled within 30 s.
  - Includes the agent's output path (40 ms reported by Chrome) and Chrome's audio jitter buffer (30–38 ms). The remote listener also hears their own output latency.
- **Video latency:** unchanged. The video jitter buffer was 5–10 ms with audio and 13 ms without, so lip sync doesn't hold the video back.
- **CPU:** the audio branch alone uses 0.6% of one core. With the full agent the difference is lost in noise.
- **Autoplay:** clicking Connect counts as user activation, so unmuted playback starts, also after a reload. Without activation Chrome blocks unmuted `play()`, which would stop the video too.

## Not tested

Whether the sound itself (not just packets) continued while on the other device and while muted, because the client was muted to avoid echo.
