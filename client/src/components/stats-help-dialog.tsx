import { useRef, type RefObject } from "react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog"
import { advancedStats, basicStats, framePacing } from "@/lib/stat-definitions"

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
            Most values update about once a second. Rates and averages use changes since the
            previous sample. — means unavailable or waiting for enough data.
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
          <StatsHelpSection title="Advanced" stats={[...advancedStats, framePacing]} />
        </div>
        <footer className="flex shrink-0 justify-end border-t px-6 py-3">
          <DialogClose render={<Button variant="outline" />}>Done</DialogClose>
        </footer>
      </DialogContent>
    </Dialog>
  )
}

function StatsHelpSection({
  title,
  stats,
}: {
  title: string
  stats: readonly { name: string; help: string }[]
}) {
  return (
    <section aria-label={`${title} statistics help`}>
      <h3 className="mb-3 text-sm font-semibold">{title}</h3>
      <dl className="space-y-4 text-sm">
        {stats.map(({ name, help }) => (
          <div key={name} className="grid gap-1 sm:grid-cols-[9rem_minmax(0,1fr)] sm:gap-4">
            <dt className="font-medium">{name}</dt>
            <dd className="leading-relaxed text-muted-foreground">{help}</dd>
          </div>
        ))}
      </dl>
    </section>
  )
}
