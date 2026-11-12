import { useState, type FormEvent } from "react"
import { CircleAlert, Monitor } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { parseAgentAddress } from "@/lib/agent-connection"

const LAST_ADDRESS_KEY = "lastAgentAddress"

interface ConnectScreenProps {
  onConnect: (address: string) => void
  error?: string
}

export function ConnectScreen({ onConnect, error }: ConnectScreenProps) {
  const [address, setAddress] = useState(() => localStorage.getItem(LAST_ADDRESS_KEY) ?? "")
  const [addressError, setAddressError] = useState<string>()

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    try {
      parseAgentAddress(address)
    } catch {
      setAddressError("Enter a host name or IP address, optionally with a port.")
      return
    }
    setAddressError(undefined)
    localStorage.setItem(LAST_ADDRESS_KEY, address.trim())
    onConnect(address.trim())
  }

  return (
    <main className="flex min-h-svh items-center justify-center bg-muted/40 p-4">
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
              <p id="agent-address-hint" className={addressError ? "text-sm text-destructive" : "text-sm text-muted-foreground"}>
                {addressError ?? "Port defaults to 8080."}
              </p>
            </div>
            <Button type="submit" size="lg" disabled={!address.trim()}>
              Connect
            </Button>
          </form>
        </CardContent>
      </Card>
    </main>
  )
}
