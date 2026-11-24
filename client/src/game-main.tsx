import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import { TooltipProvider } from "@/components/ui/tooltip"
import { GameViewer } from "@/components/game-viewer"
import "./index.css"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider>
      <GameViewer />
    </TooltipProvider>
  </StrictMode>,
)
