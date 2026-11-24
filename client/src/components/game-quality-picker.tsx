import { Slider } from "@base-ui/react/slider"
import { ChevronDown } from "lucide-react"
import { useState, type RefObject } from "react"

import { Button } from "@/components/ui/button"
import { Popover, PopoverContent, PopoverTitle, PopoverTrigger } from "@/components/ui/popover"
import { gameQualityPresets } from "@/lib/game-quality"

interface GameQualityPickerProps {
  container: RefObject<HTMLElement | null>
  value: number
  connected: boolean
  canChange: boolean
  pending?: boolean
  onChange: (value: number) => void
}

export function GameQualityPicker({
  container,
  value,
  connected,
  canChange,
  pending,
  onChange,
}: GameQualityPickerProps) {
  const [preview, setPreview] = useState<number>()
  const selected = preview ?? value

  return (
    <Popover onOpenChange={(open) => !open && setPreview(undefined)}>
      <PopoverTrigger disabled={!connected} render={<Button variant="ghost" />}>
        Quality
        <ChevronDown
          aria-hidden
          className="size-3.5 text-muted-foreground transition-transform group-aria-expanded/button:rotate-180 motion-reduce:transition-none"
        />
      </PopoverTrigger>
      <PopoverContent
        container={container}
        align="start"
        sideOffset={6}
        className="w-44 max-w-[calc(100vw-1rem)] gap-1.5 rounded-2xl px-3.5 py-3"
      >
        <PopoverTitle className="sr-only">Stream quality</PopoverTitle>
        <p className="text-center font-medium">{gameQualityPresets[selected].label}</p>
        <Slider.Root
          value={selected}
          max={gameQualityPresets.length - 1}
          thumbAlignment="edge"
          disabled={!connected || !canChange}
          aria-disabled={pending || undefined}
          onValueChange={(next, details) => {
            if (pending) details.cancel()
            else setPreview(next)
          }}
          onValueCommitted={(next) => {
            setPreview(undefined)
            if (!pending && next !== value) onChange(next)
          }}
          className="data-disabled:opacity-50"
        >
          <Slider.Control className="relative flex h-9 touch-none items-center">
            <Slider.Track className="relative h-5 w-full overflow-hidden rounded-full bg-muted ring-1 ring-border ring-inset">
              <Slider.Indicator className="bg-primary" />
              <div
                aria-hidden
                className="pointer-events-none absolute inset-x-2.5 inset-y-0 flex items-center justify-between"
              >
                {gameQualityPresets.map(({ label }) => (
                  <span key={label} className="size-1 rounded-full bg-muted-foreground" />
                ))}
              </div>
            </Slider.Track>
            <Slider.Thumb
              aria-label="Stream quality"
              aria-valuetext={gameQualityPresets[selected].label}
              className="size-6 rounded-full border border-primary bg-primary-foreground shadow-sm has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring has-[:focus-visible]:ring-offset-2 has-[:focus-visible]:ring-offset-popover"
            />
          </Slider.Control>
        </Slider.Root>
      </PopoverContent>
    </Popover>
  )
}
