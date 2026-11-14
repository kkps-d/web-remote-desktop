import { Keyboard } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { InputSender } from "@/lib/input-sender"

// Shortcuts the browser or client OS keeps for itself, as `KeyboardEvent.code`s in press order.
const SHORTCUTS = [
  { label: "Win", codes: ["MetaLeft"] },
  { label: "Alt+Tab", codes: ["AltLeft", "Tab"] },
  { label: "Alt+F4", codes: ["AltLeft", "F4"] },
  { label: "Ctrl+W", codes: ["ControlLeft", "KeyW"] },
  { label: "Ctrl+Shift+Esc", codes: ["ControlLeft", "ShiftLeft", "Escape"] },
]

interface ShortcutsMenuProps {
  sender?: InputSender
  disabled: boolean
}

/** Toolbar menu that sends key combinations the browser wouldn't pass through. */
export function ShortcutsMenu({ sender, disabled }: ShortcutsMenuProps) {
  function press(codes: string[]) {
    if (!sender) return
    for (const code of codes) sender.key(code, true)
    for (const code of codes.toReversed()) sender.key(code, false)
  }

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger
          render={
            <DropdownMenuTrigger
              disabled={disabled}
              render={<Button variant="ghost" size="icon" aria-label="Send shortcut" />}
            />
          }
        >
          <Keyboard aria-hidden />
        </TooltipTrigger>
        <TooltipContent>Send shortcut</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="end" className="w-auto min-w-40">
        {SHORTCUTS.map(({ label, codes }) => (
          <DropdownMenuItem key={label} onClick={() => press(codes)}>
            {label}
          </DropdownMenuItem>
        ))}
        {/* SendInput can't produce Ctrl+Alt+Del; only SendSAS from a service can. */}
        <Tooltip>
          <TooltipTrigger
            // Disabled items ignore the pointer by default, which would hide the tooltip.
            render={<DropdownMenuItem disabled className="data-disabled:pointer-events-auto" />}
          >
            Ctrl+Alt+Del
          </TooltipTrigger>
          <TooltipContent side="left">Needs the agent to run as a Windows service</TooltipContent>
        </Tooltip>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
