// The statistics the overlay shows, in display order, with the help text that explains each one.

export const basicStats = [
  { name: "Stream", help: "Received video width × height in pixels, reported by the browser." },
  {
    name: "Bitrate",
    help: "Video data received per second: new bytes × 8 ÷ elapsed time, shown in kbps or Mbps.",
  },
  {
    name: "Framerate",
    help: "Frames decoded per second, reported by the browser; otherwise new decoded frames ÷ elapsed seconds.",
  },
  {
    name: "Net. RTT",
    help: "Network round-trip time on the active connection, reported in seconds and converted to ms. This excludes capture, encoding, and display delay.",
  },
] as const

export const advancedStats = [
  {
    name: "Source",
    help: "Captured desktop width × height in pixels, reported by the agent before stream scaling.",
  },
  {
    name: "Encode time",
    help: "Average time from encoder input to output, including queueing: added encoding time ÷ newly encoded frames, in ms/frame.",
  },
  {
    name: "Decode time",
    help: "Average time to decode a frame: added decoding time ÷ newly decoded frames, converted to ms/frame.",
  },
  {
    name: "Packet loss",
    help: "Percentage of video packets reported missing: new lost packets ÷ (new received + new lost packets) × 100.",
  },
  {
    name: "Jitter",
    help: "The browser’s smoothed estimate of variation in packet arrival timing, converted from seconds to ms.",
  },
  {
    name: "Jitter buffer delay",
    help: "Average time frames wait in the receiver’s buffer: added buffer time ÷ newly emitted frames, converted to ms.",
  },
  {
    name: "Dropped frames",
    help: "Total video frames discarded before decoding or display, counted by the browser for this stream.",
  },
  {
    name: "Freezes",
    help: "Browser-counted playback freezes and their total duration for this stream. A freeze is a frame gap ≥ the larger of 3× the recent average gap or that average + 150 ms.",
  },
  {
    name: "Codec",
    help: "Video compression format, such as H264, read from the received stream’s negotiated codec.",
  },
  {
    name: "Decoder",
    help: "Name of the video decoder implementation reported by the browser. Chromium-based browsers hide it unless the page is using a camera or microphone.",
  },
] as const

export type StatDefinition = (typeof basicStats)[number] | (typeof advancedStats)[number]
export type StatName = StatDefinition["name"]
