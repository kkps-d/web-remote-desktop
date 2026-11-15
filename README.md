# Web Remote Desktop

A simple remote desktop app that consists of a web client and a agent.

The web client connects to the agent installed on a computer to remote desktop.

See [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) for the design. LAN/VPN only, plain HTTP. Streams the desktop with audio and takes mouse and keyboard input (v1.1).

See [Known issues](docs/KNOWN_ISSUES.md) for current limitations and workarounds.

## Running

**Agent** (Windows): there are two agents with the same API. The Rust agent in `agent-rs/` replaces the C# agent in `agent/`, which is archived: kept for reference and no longer updated. Both need [GStreamer](https://gstreamer.freedesktop.org/download/) 1.28.4+ (MSVC build). Audio needs Windows 10 build 20348 or later (e.g. Windows 11); older builds stream video only.

Both listen on port 8080 on all interfaces. Run one at a time, or give one another port. Allow the agent through Windows Firewall on private networks.

_Rust agent_: needs Windows 10 1809 or later, [Rust](https://rustup.rs/) with the MSVC toolchain, and GStreamer's development files (pick "Runtime and development headers" in the installer). The build finds GStreamer through pkg-config, and Windows loads GStreamer's DLLs before the agent starts, so set these once for your user (paths of the per-user installer), then open a new terminal:

```powershell
$gst = "$env:LOCALAPPDATA\Programs\gstreamer\1.0\msvc_x86_64"
[Environment]::SetEnvironmentVariable('PKG_CONFIG', "$gst\bin\pkg-config.exe", 'User')
[Environment]::SetEnvironmentVariable('PKG_CONFIG_PATH', "$gst\lib\pkgconfig", 'User')
[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ";$gst\bin", 'User')
```

```bash
cargo run --release --manifest-path agent-rs/Cargo.toml
```

Change the port with `-- --port 9000`. Set `RUST_LOG=debug` for more detail.

_C# agent_: needs the [.NET 10 SDK](https://dotnet.microsoft.com/). The GStreamer runtime is enough.

```bash
dotnet run --project agent
```

Change the port in `agent/appsettings.json` or with `-- --Port 9000`.

**Web client**: needs Node.js.

```bash
cd client
npm install
npm run dev
```

Open http://localhost:5173 and enter the agent's address (e.g. `192.168.1.20`). `npm run build` produces a static site in `client/dist`; serve it over plain HTTP, since HTTPS pages can't reach the HTTP agent.

## Testing

```bash
cargo test --manifest-path agent-rs/Cargo.toml
```

Run `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` in `agent-rs/` too.

```bash
dotnet test agent.Tests
```

```bash
npm --prefix client test
```

Stop a running agent first; its build output is locked while it runs.
