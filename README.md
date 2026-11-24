# Web Remote Desktop

A simple remote desktop app: a web client that connects to an agent running on the computer you want to control.

- Streams the desktop with audio
- Takes mouse and keyboard input
- LAN/VPN only, plain HTTP

## Docs

- [Architecture](docs/ARCHITECTURE.md): how the client and agent work
- [Roadmap](docs/ROADMAP.md): milestones, planned work and open questions
- [Known issues](docs/KNOWN_ISSUES.md): current limitations and workarounds
- [Linux development](docs/LINUX_DEV.md): the Linux VM setup
- [Encoders](docs/ENCODERS.md): Linux and Windows encoder selection and benchmarks
- [Spikes](docs/spikes/): measurements from the data channel and audio spikes
- [Design guidelines](docs/DESIGN_GUIDELINES.md): coding rules for the client and agent

## Running

### Agent

The Rust agent in `agent/` runs on Windows and Linux. It listens on port 8080 on all interfaces.

#### Windows

Requirements:

- Windows 10 1809 or later (audio needs build 20348 or later, e.g. Windows 11; older builds stream video only)
- [Rust](https://rustup.rs/) with the MSVC toolchain
- [GStreamer](https://gstreamer.freedesktop.org/download/) 1.28.4+ (MSVC build). In the installer, pick "Runtime and development headers".

Setup:

1. Set these environment variables once for your user, then open a new terminal. The build finds GStreamer through pkg-config, and Windows has to load GStreamer's DLLs before the agent starts. The paths are for the per-user installer.

   ```powershell
   $gst = "$env:LOCALAPPDATA\Programs\gstreamer\1.0\msvc_x86_64"
   [Environment]::SetEnvironmentVariable('PKG_CONFIG', "$gst\bin\pkg-config.exe", 'User')
   [Environment]::SetEnvironmentVariable('PKG_CONFIG_PATH', "$gst\lib\pkgconfig", 'User')
   [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ";$gst\bin", 'User')
   ```

2. Allow the agent through Windows Firewall on private networks.
3. Run the agent:

   ```powershell
   cargo run --release --manifest-path agent/Cargo.toml
   ```

#### Linux

Requirements:

- A GNOME desktop on Wayland with PipeWire (tested on Ubuntu 26.04)
- The [packages](docs/LINUX_DEV.md#packages) and Rust
- [Access to `/dev/uinput`](docs/LINUX_DEV.md#input-access) for your user, for input

Run the agent from a terminal in the logged-in desktop session, not over SSH:

```bash
cargo run --release --manifest-path agent/Cargo.toml
```

The first viewer brings up GNOME's screen sharing dialog on the Linux desktop. After you allow it, later viewers and restarts skip the dialog.

For gamescope, set `[capture]` and run the agent in the compositor’s PipeWire
session. Set `eis_socket` and install `libei1` (1.4 or later) to enable keyboard
and mouse input. Set `pipewire_audio_sink` to stream a named sink monitor. See the [example config](agent/agent.example.toml).

#### Options

| Option | How |
| --- | --- |
| Port | `-- --port 9000` |
| Verbose logs | `RUST_LOG=debug` |
| Connection modes, stream defaults and limits | Copy [`agent.example.toml`](agent/agent.example.toml) to `agent.toml` next to the executable, or pass `-- --config <path>` |

### Web client

Requirements: Node.js.

```bash
cd client
npm install
npm run dev
```

Open http://localhost:5173 and enter the agent's address (e.g. `192.168.1.20`).

`npm run build` produces a static site in `client/dist`. The game viewer at
`/play/` takes `?agentPort=PORT` for an agent on the viewer's host or
`?localAgent=127.0.0.1:PORT` for a loopback agent.

> [!NOTE]
> Serve the client over plain HTTP. HTTPS pages can't reach the HTTP agent.

## Testing

> [!NOTE]
> Stop any running agent first. Its build output is locked while it runs.

```bash
cargo test --manifest-path agent/Cargo.toml
cargo fmt --check --manifest-path agent/Cargo.toml
cargo clippy --manifest-path agent/Cargo.toml --all-targets -- -D warnings
npm --prefix client test
npm --prefix client run format:check
npm --prefix client run lint
npm --prefix client run build
```
