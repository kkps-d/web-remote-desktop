import { useId } from "react"
import { LoaderCircle, Settings2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { Popover, PopoverContent, PopoverTitle, PopoverTrigger } from "@/components/ui/popover"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { scaleToHeight, type Resolution, type StreamInfo, type StreamSettings } from "@/lib/agent-connection"

const FRAMERATES = [15, 30, 60, 120]
const BITRATES_KBPS = [2000, 4000, 8000, 15000, 25000]
// Common heights offered below the source's own resolution.
const SCALED_HEIGHTS = [2160, 1440, 1080, 720, 480]

interface StreamSettingsPickerProps {
  info?: StreamInfo
  error?: string
  disabled: boolean
  onChange: (change: Partial<StreamSettings>) => void
}

/** Toolbar popover for picking the stream's resolution, frame rate, bitrate and encoder. */
export function StreamSettingsPicker({ info, error, disabled, onChange }: StreamSettingsPickerProps) {
  const label = "Stream settings"

  return (
    <Popover>
      <Tooltip>
        <TooltipTrigger
          render={
            <PopoverTrigger
              disabled={disabled}
              render={<Button variant="ghost" size="icon" aria-label={label} />}
            />
          }
        >
          <Settings2 />
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>

      <PopoverContent align="end" className="gap-3">
        <PopoverTitle>{label}</PopoverTitle>
        {info ? (
          <SettingsFields info={info} onChange={onChange} />
        ) : (
          !error && (
            <p role="status" className="flex items-center gap-2 text-muted-foreground">
              <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden />
              Loading…
            </p>
          )
        )}
        {error && (
          <p role="alert" className="text-destructive">
            {error}
          </p>
        )}
      </PopoverContent>
    </Popover>
  )
}

function SettingsFields({ info, onChange }: { info: StreamInfo; onChange: StreamSettingsPickerProps["onChange"] }) {
  const { settings, source, limits } = info
  const resolutions = resolutionOptions(source, limits.resolution).map((size) => ({
    value: resolutionKey(size),
    label: `${size.width}×${size.height}${resolutionKey(size) === resolutionKey(source) ? " (native)" : ""}`,
  }))
  const framerates = choices(FRAMERATES, limits.framerate, settings.framerate).map((fps) => ({
    value: String(fps),
    label: `${fps} fps`,
  }))
  const bitrates = choices(BITRATES_KBPS, limits.bitrateKbps, settings.bitrateKbps).map((kbps) => ({
    value: String(kbps),
    label: `${kbps / 1000} Mbps`,
  }))
  const encoders = info.encoders.map((encoder) => ({
    value: encoder.id,
    label: encoder.label,
    disabled: !encoder.available,
    description: encoder.available ? undefined : (encoder.unavailableReason ?? "Not available"),
  }))

  return (
    <>
      <SettingField
        label="Resolution"
        items={resolutions}
        value={resolutionKey(settings)}
        onChange={(value) => {
          const [width, height] = value.split("x").map(Number)
          onChange({ width, height })
        }}
      />
      <SettingField
        label="Frame rate"
        items={framerates}
        value={String(settings.framerate)}
        onChange={(value) => onChange({ framerate: Number(value) })}
      />
      <SettingField
        label="Bitrate"
        items={bitrates}
        value={String(settings.bitrateKbps)}
        onChange={(value) => onChange({ bitrateKbps: Number(value) })}
      />
      <SettingField
        label="Encoder"
        items={encoders}
        value={settings.encoder}
        onChange={(encoder) => onChange({ encoder })}
      />
    </>
  )
}

interface SettingItem {
  value: string
  label: string
  disabled?: boolean
  /** Shown under the label, e.g. why the item is disabled. */
  description?: string
}

function SettingField({
  label,
  items,
  value,
  onChange,
}: {
  label: string
  items: SettingItem[]
  value: string
  onChange: (value: string) => void
}) {
  const id = useId()

  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Select items={items} value={value} onValueChange={(next) => next !== null && onChange(next)}>
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {items.map((item) => (
            <SelectItem key={item.value} value={item.value} disabled={item.disabled}>
              {item.description ? (
                <span className="flex flex-col">
                  {item.label}
                  <span className="text-xs whitespace-normal text-muted-foreground">{item.description}</span>
                </span>
              ) : (
                item.label
              )}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

/** The largest size the agent allows (usually the source) plus common smaller sizes with the same aspect ratio. */
function resolutionOptions(source: Resolution, largest: Resolution): Resolution[] {
  const scaled = SCALED_HEIGHTS.filter((height) => height < largest.height).map((height) => scaleToHeight(source, height))
  return [largest, ...scaled]
}

/** The presets below the agent's limit, the limit itself and the current value, ascending. */
function choices(presets: number[], max: number, current: number): number[] {
  const values = new Set([...presets.filter((value) => value < max), max, current])
  return [...values].sort((a, b) => a - b)
}

function resolutionKey({ width, height }: Resolution) {
  return `${width}x${height}`
}
