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
import { INPUT_RATES_HZ, type InputSettings } from "@/hooks/use-remote-input"
import type { PointerMode } from "@/lib/pointer-input"

interface InputMenuProps extends InputSettings {
  disabled: boolean
  onModeChange: (mode: PointerMode) => void
  onRateChange: (rateHz: number) => void
}

/** Toolbar menu for the pointer mode and how often pointer input is sent. */
export function InputMenu({ mode, rateHz, disabled, onModeChange, onRateChange }: InputMenuProps) {
  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger
          render={
            <DropdownMenuTrigger
              disabled={disabled}
              render={<Button variant="ghost" size="icon" aria-label="Mouse input" />}
            />
          }
        >
          <MousePointer2 aria-hidden />
        </TooltipTrigger>
        <TooltipContent>Mouse input</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="end" className="w-auto min-w-40">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Pointer</DropdownMenuLabel>
          <DropdownMenuRadioGroup value={mode} onValueChange={onModeChange}>
            <DropdownMenuRadioItem value="absolute">Absolute</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="relative">Relative</DropdownMenuRadioItem>
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
