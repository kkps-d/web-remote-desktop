import { MousePointer2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { MouseMode, TouchMode } from "@/lib/pointer-input"
import { INPUT_RATES_HZ, type InputSettings } from "@/lib/saved-settings"

interface InputMenuProps extends InputSettings {
  disabled: boolean
  touchSupported: boolean
  onMouseChange: (mouse: MouseMode) => void
  onTouchChange: (touch: TouchMode) => void
  onRateChange: (rateHz: number) => void
}

/** Toolbar menu for the mouse and touch modes and how often pointer input is sent. */
export function InputMenu({
  mouse,
  touch,
  rateHz,
  disabled,
  touchSupported,
  onMouseChange,
  onTouchChange,
  onRateChange,
}: InputMenuProps) {
  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger
          render={
            <DropdownMenuTrigger
              disabled={disabled}
              render={<Button variant="ghost" size="icon" aria-label="Input" />}
            />
          }
        >
          <MousePointer2 aria-hidden />
        </TooltipTrigger>
        <TooltipContent>Input</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="end" className="w-auto min-w-40">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Mouse</DropdownMenuLabel>
          <DropdownMenuRadioGroup value={mouse} onValueChange={onMouseChange}>
            <DropdownMenuRadioItem value="absolute">Absolute</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="relative">Relative</DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuLabel>Touch</DropdownMenuLabel>
          <DropdownMenuRadioGroup value={touch} onValueChange={onTouchChange}>
            <DropdownMenuRadioItem value="touchscreen" disabled={!touchSupported}>
              {touchSupported ? "Touchscreen" : "Touchscreen unavailable"}
            </DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="direct">Direct</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="trackpad">Trackpad</DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuLabel>Rate</DropdownMenuLabel>
          <DropdownMenuRadioGroup value={rateHz} onValueChange={onRateChange}>
            {INPUT_RATES_HZ.map((rate) => (
              <DropdownMenuRadioItem key={rate} value={rate}>
                {rate} Hz
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
