import { useRef, type RefObject } from "react"

import { Button } from "@/components/ui/button"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"

const basicStats = [
  ["Stream", "Received video width × height in pixels, reported by the browser."],
  ["Bitrate", "Video data received per second: new bytes × 8 ÷ elapsed time, shown in kbps or Mbps."],
  ["Framerate", "Frames decoded per second, reported by the browser; otherwise new decoded frames ÷ elapsed seconds."],
  ["Net. RTT", "Network round-trip time on the active connection, reported in seconds and converted to ms. This excludes capture, encoding, and display delay."],
]

const advancedStats = [
  ["Source", "Captured desktop width × height in pixels, reported by the agent before stream scaling."],
  ["Encode time", "Average time from encoder input to output, including queueing: added encoding time ÷ newly encoded frames, in ms/frame."],
  ["Decode time", "Average time to decode a frame: added decoding time ÷ newly decoded frames, converted to ms/frame."],
  ["Packet loss", "Percentage of video packets reported missing: new lost packets ÷ (new received + new lost packets) × 100."],
  ["Jitter", "The browser’s smoothed estimate of variation in packet arrival timing, converted from seconds to ms."],
  ["Jitter buffer delay", "Average time frames wait in the receiver’s buffer: added buffer time ÷ newly emitted frames, converted to ms."],
  ["Dropped frames", "Total video frames discarded before decoding or display, counted by the browser for this stream."],
  ["Freezes", "Browser-counted playback freezes and their total duration for this stream. A freeze is a frame gap ≥ the larger of 3× the recent average gap or that average + 150 ms."],
  ["Codec", "Video compression format, such as H264, read from the received stream’s negotiated codec."],
  ["Decoder", "Name of the video decoder implementation reported by the browser, when available."],
]

interface StatsHelpDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  returnFocus: RefObject<HTMLElement | null>
}

export function StatsHelpDialog({ open, onOpenChange, returnFocus }: StatsHelpDialogProps) {
  const titleRef = useRef<HTMLHeadingElement>(null)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        initialFocus={titleRef}
        finalFocus={returnFocus}
        className="flex max-h-[calc(100dvh-2rem)] max-w-2xl flex-col gap-0 overflow-hidden p-0"
      >
        <header className="shrink-0 border-b px-6 py-4 pr-14">
          <DialogTitle ref={titleRef} tabIndex={-1} className="text-lg font-semibold outline-none">
            Statistics help
          </DialogTitle>
          <DialogDescription className="mt-1 text-sm text-muted-foreground">
            Updated about once a second. Rates and averages use changes since the previous sample.
            {" "}— means unavailable or waiting for enough data.
          </DialogDescription>
        </header>
        <div
          role="region"
          aria-label="Statistics explanations"
          tabIndex={0}
          className="min-h-0 overflow-y-auto overscroll-contain px-6 py-4 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
        >
          <StatsHelpSection title="Basic" stats={basicStats} />
          <div className="my-5 border-t" />
          <StatsHelpSection title="Advanced" stats={advancedStats} />
        </div>
        <footer className="flex shrink-0 justify-end border-t px-6 py-3">
          <DialogClose render={<Button variant="outline" />}>Done</DialogClose>
        </footer>
      </DialogContent>
    </Dialog>
  )
}

function StatsHelpSection({ title, stats }: { title: string; stats: string[][] }) {
  return (
    <section aria-label={`${title} statistics help`}>
      <h3 className="mb-3 text-sm font-semibold">{title}</h3>
      <dl className="space-y-4 text-sm">
        {stats.map(([name, explanation]) => (
          <div key={name} className="grid gap-1 sm:grid-cols-[9rem_minmax(0,1fr)] sm:gap-4">
            <dt className="font-medium">{name}</dt>
            <dd className="leading-relaxed text-muted-foreground">{explanation}</dd>
          </div>
        ))}
      </dl>
    </section>
  )
}
