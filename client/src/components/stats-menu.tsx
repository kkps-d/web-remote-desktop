import { useRef, useState } from "react"
import { Activity, Check, CircleHelp } from "lucide-react"
import { Menu } from "@base-ui/react/menu"

import { Button } from "@/components/ui/button"
import { StatsHelpDialog } from "@/components/stats-help-dialog"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

interface StatsMenuProps {
  basic: boolean
  advanced: boolean
  onBasicChange: (checked: boolean) => void
  onAdvancedChange: (checked: boolean) => void
}

export function StatsMenu({ basic, advanced, onBasicChange, onAdvancedChange }: StatsMenuProps) {
  const [helpOpen, setHelpOpen] = useState(false)
  const triggerRef = useRef<HTMLButtonElement>(null)

  return (
    <>
      <Menu.Root>
        <Tooltip>
          <TooltipTrigger render={<Menu.Trigger ref={triggerRef} render={<Button variant="ghost" size="icon" aria-label="Statistics" />} />}>
            <Activity aria-hidden />
          </TooltipTrigger>
          <TooltipContent>Statistics</TooltipContent>
        </Tooltip>
        <Menu.Portal>
          <Menu.Positioner align="end" sideOffset={4} className="z-50">
            <Menu.Popup className="min-w-40 rounded-lg bg-popover p-1 text-sm text-popover-foreground shadow-md ring-1 ring-foreground/10 outline-hidden">
              <StatsToggle checked={basic} onCheckedChange={onBasicChange}>Basic</StatsToggle>
              <StatsToggle checked={advanced} onCheckedChange={onAdvancedChange}>Advanced</StatsToggle>
              <Menu.Separator className="my-1 h-px bg-border" />
              <Menu.Item
                onClick={() => setHelpOpen(true)}
                className="flex cursor-default select-none items-center gap-2 rounded-md px-2 py-1.5 outline-hidden data-highlighted:bg-accent data-highlighted:text-accent-foreground"
              >
                <CircleHelp className="size-4" aria-hidden />
                Help
              </Menu.Item>
            </Menu.Popup>
          </Menu.Positioner>
        </Menu.Portal>
      </Menu.Root>
      <StatsHelpDialog open={helpOpen} onOpenChange={setHelpOpen} returnFocus={triggerRef} />
    </>
  )
}

function StatsToggle({ children, ...props }: Menu.CheckboxItem.Props) {
  return (
    <Menu.CheckboxItem {...props} closeOnClick={false} className="relative flex cursor-default select-none items-center rounded-md py-1.5 pr-2 pl-8 outline-hidden data-highlighted:bg-accent data-highlighted:text-accent-foreground">
      <Menu.CheckboxItemIndicator className="absolute left-2"><Check className="size-4" /></Menu.CheckboxItemIndicator>
      {children}
    </Menu.CheckboxItem>
  )
}
