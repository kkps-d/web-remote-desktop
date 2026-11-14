# Web Remote Desktop

A simple remote desktop app that consists of a web client and a agent.

The web client connects to the agent installed on a computer to remote desktop.

See [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) for the design. LAN/VPN only, plain HTTP. Streams the desktop with audio and takes mouse and keyboard input (v1.1).

See [Known issues](docs/KNOWN_ISSUES.md) for current limitations and workarounds.

## Running

**Agent** (Windows): needs the [.NET 10 SDK](https://dotnet.microsoft.com/) and [GStreamer](https://gstreamer.freedesktop.org/download/) 1.28.4+ (MSVC build, "Complete" install). Audio needs Windows 10 build 20348 or later (e.g. Windows 11); older builds stream video only.

```bash
dotnet run --project agent
```

Listens on port 8080 on all interfaces; change it in `agent/appsettings.json` or with `-- --Port 9000`. Allow it through Windows Firewall on private networks.

**Web client**: needs Node.js.

```bash
cd client
npm install
npm run dev
```

Open http://localhost:5173 and enter the agent's address (e.g. `192.168.1.20`). `npm run build` produces a static site in `client/dist`; serve it over plain HTTP, since HTTPS pages can't reach the HTTP agent.

## Testing

```bash
dotnet test agent.Tests
```

```bash
npm --prefix client test
```

Stop a running agent first; its build output is locked while it runs.
