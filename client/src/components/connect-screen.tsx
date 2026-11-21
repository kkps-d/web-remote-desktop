import { useState, type FormEvent } from "react"
import { CircleAlert, Monitor, X } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { parseAgentAddress } from "@/lib/agent-connection"
import { forgetDevice, readRecentDevices, type RecentDevice } from "@/lib/recent-devices"
import { readStorage, writeStorage } from "@/lib/storage"

const LAST_ADDRESS_KEY = "lastAgentAddress"

interface ConnectScreenProps {
  onConnect: (address: string) => void
  error?: string
}

export function ConnectScreen({ onConnect, error }: ConnectScreenProps) {
  const [address, setAddress] = useState(() => readStorage(LAST_ADDRESS_KEY) ?? "")
  const [addressError, setAddressError] = useState<string>()
  const [devices, setDevices] = useState(readRecentDevices)

  function connectTo(target: string) {
    setAddress(target)
    setAddressError(undefined)
    writeStorage(LAST_ADDRESS_KEY, target)
    onConnect(target)
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    try {
      parseAgentAddress(address)
    } catch {
      setAddressError("Enter a host name or IP address, optionally with a port.")
      return
    }
    connectTo(address.trim())
  }

  return (
    <main className="flex min-h-svh flex-col items-center justify-center gap-8 bg-muted/40 p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-lg">
            <Monitor aria-hidden />
            Remote Desktop
          </CardTitle>
          <CardDescription>Connect to an agent on your network.</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} noValidate className="flex flex-col gap-4">
            {error && (
              <Alert variant="destructive">
                <CircleAlert aria-hidden />
                <AlertTitle>Couldn't connect</AlertTitle>
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}
            <div className="flex flex-col gap-2">
              <Label htmlFor="agent-address">Agent address</Label>
              <Input
                id="agent-address"
                value={address}
                onChange={(event) => setAddress(event.target.value)}
                placeholder="192.168.1.20:8080"
                autoComplete="off"
                spellCheck={false}
                required
                autoFocus
                aria-invalid={addressError ? true : undefined}
                aria-describedby="agent-address-hint"
              />
              <p
                id="agent-address-hint"
                className={
                  addressError ? "text-sm text-destructive" : "text-sm text-muted-foreground"
                }
              >
                {addressError ?? "Port defaults to 8080."}
              </p>
            </div>
            <Button type="submit" size="lg" disabled={!address.trim()}>
              Connect
            </Button>
          </form>
        </CardContent>
      </Card>

      {devices.length > 0 && (
        <RecentDevices
          devices={devices}
          onConnect={connectTo}
          onForget={(device) => setDevices(forgetDevice(device))}
        />
      )}
    </main>
  )
}

function RecentDevices({
  devices,
  onConnect,
  onForget,
}: {
  devices: RecentDevice[]
  onConnect: (address: string) => void
  onForget: (address: string) => void
}) {
  // Read once so the times stay stable across re-renders.
  const [now] = useState(Date.now)

  return (
    <section
      aria-labelledby="recent-devices-heading"
      className="flex w-full max-w-[52rem] flex-col gap-3"
    >
      <h2
        id="recent-devices-heading"
        className="text-center text-sm font-medium text-muted-foreground"
      >
        Recent devices
      </h2>
      {/* Centered rows so a few devices stay balanced under the form. */}
      <ul className="flex flex-wrap justify-center gap-4">
        {devices.map((device, index) => (
          <li key={device.address} className="w-full sm:w-64">
            {/* The connect button's ::after stretches over the whole card so all of it is clickable. */}
            <Card className="relative gap-0 py-0 transition-shadow hover:ring-foreground/25 has-[[data-connect]:focus-visible]:ring-3 has-[[data-connect]:focus-visible]:ring-ring/50">
              <div className="flex aspect-video items-center justify-center bg-muted">
                {device.thumbnail ? (
                  <img src={device.thumbnail} alt="" className="size-full object-cover" />
                ) : (
                  <Monitor className="size-8 text-muted-foreground" aria-hidden />
                )}
              </div>
              <div className="flex items-center gap-2 p-3">
                <div className="min-w-0 flex-1">
                  <button
                    type="button"
                    data-connect
                    aria-label={`Connect to ${device.address}`}
                    title={device.address}
                    aria-describedby={`recent-device-${index}-time`}
                    onClick={() => onConnect(device.address)}
                    className="block max-w-full truncate text-left font-medium outline-none after:absolute after:inset-0"
                  >
                    {device.address}
                  </button>
                  <p id={`recent-device-${index}-time`} className="text-xs text-muted-foreground">
                    Last connected {timeAgo(device.lastConnectedAt, now)}
                  </p>
                </div>
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        className="relative"
                        aria-label={`Forget ${device.address}`}
                        onClick={() => onForget(device.address)}
                      />
                    }
                  >
                    <X />
                  </TooltipTrigger>
                  <TooltipContent>Forget</TooltipContent>
                </Tooltip>
              </div>
            </Card>
          </li>
        ))}
      </ul>
    </section>
  )
}

const timeUnits: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["week", 7 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
]
const relativeTime = new Intl.RelativeTimeFormat("en", { numeric: "auto" })

function timeAgo(timestamp: number, now: number) {
  const seconds = (timestamp - now) / 1000
  for (const [unit, size] of timeUnits) {
    if (Math.abs(seconds) >= size) return relativeTime.format(Math.trunc(seconds / size), unit)
  }
  return "just now"
}
