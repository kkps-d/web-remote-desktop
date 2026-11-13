import { useRef, useState } from "react"
import { Activity, CircleHelp } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
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
      <DropdownMenu>
        <Tooltip>
          <TooltipTrigger
            render={
              <DropdownMenuTrigger
                ref={triggerRef}
                render={<Button variant="ghost" size="icon" aria-label="Statistics" />}
              />
            }
          >
            <Activity aria-hidden />
          </TooltipTrigger>
          <TooltipContent>Statistics</TooltipContent>
        </Tooltip>
        <DropdownMenuContent align="end" className="w-auto min-w-40">
          <DropdownMenuCheckboxItem checked={basic} onCheckedChange={onBasicChange}>
            Basic
          </DropdownMenuCheckboxItem>
          <DropdownMenuCheckboxItem checked={advanced} onCheckedChange={onAdvancedChange}>
            Advanced
          </DropdownMenuCheckboxItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={() => setHelpOpen(true)}>
            <CircleHelp aria-hidden />
            Help
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <StatsHelpDialog open={helpOpen} onOpenChange={setHelpOpen} returnFocus={triggerRef} />
    </>
  )
}
