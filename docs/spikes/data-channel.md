# Data channel spike

Branch `spike/data-channel`, Windows. Checked that `webrtcbin` data channels can carry input. Tested on localhost with a 2560×1440 display at 100% scaling.

## Results

- `webrtcbin` data channels work. Connect `on-data-channel` before setting the remote description.
- Round trip (ping echo): 0.6 ms median. A burst of 2,000 JSON messages, each injected with `SendInput`, was handled in about 100 ms.
- Absolute and relative moves landed on the exact pixel. Relative moves skipped acceleration only because "Enhance pointer precision" was off; to keep 1:1 movement with it on, apply deltas to the current cursor position and move absolutely.
- Absolute moves can land 1 px off (1480 → 1479); map fractions to pixel centers.
- `show-cursor` on `d3d11screencapturesrc` can be toggled while streaming.
- Closing the peer connection makes `sctpenc` post a pipeline error ("SCTP association went into error state"), which ends the session as a failure. Treat it as the channel closing.
- Over plain HTTP from a LAN address, Chrome lacks `pointerrawupdate` and `getCoalescedEvents` (secure context only). `pointermove` then arrives once per display frame, so a real mouse sends at most the display's refresh rate.

## Rate caps

Fed 2,000 synthetic moves/s for 5 s. Agent CPU is % of one core; capture and encode alone: 2.2%.

| Cap | Sent/s (abs / rel) | Agent CPU (abs / rel) |
| --- | --- | --- |
| 125 Hz | 120 / 120 | 6.0% / 9.3% |
| 250 Hz | 230 / 234 | 6.9% / 8.8% |
| 500 Hz | 427 / 429 | 14.0% / 14.5% |
| 1000 Hz | 748 / 762 | 17.0% / 11.8% |
| None | 2,000 / 1,392 | 41.4% / 14.8% |

- The agent takes 120–180 µs per real move, mostly `SendInput`.
- Ping round trips stayed at 0.3 ms median, 0.7 ms p95 or less, at every rate.
- Merging lost nothing: relative circles ended on the same pixel at every cap.
- Browser timers can't wait less than about 1 ms, so the 500 and 1000 Hz caps fall short.

## Not tested

Button clicks (to avoid clicking on the live desktop), keyboard, touch, Pointer Lock, display scaling other than 100%.
