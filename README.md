# Web Remote Desktop

A simple remote desktop app that consists of a web client and a agent.

The web client connects to the agent installed on a computer to remote desktop.

See [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) for the design. LAN/VPN only, plain HTTP, view only (v0.5).

## Running

**Agent** (Windows): needs the [.NET 10 SDK](https://dotnet.microsoft.com/) and [GStreamer](https://gstreamer.freedesktop.org/download/) 1.28+ (MSVC build, "Complete" install).

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
