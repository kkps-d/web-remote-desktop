//! The agent's settings file, `agent.toml`, and its command-line arguments: which connections
//! the agent accepts, and the stream's defaults and limits. See `agent.example.toml`.

use std::io;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::stream_settings::{Resolution, StreamConfig};

pub const DEFAULT_PORT: u16 = 8080;
const FILE_NAME: &str = "agent.toml";
const USAGE: &str = "Usage: agent [--config agent.toml] [--port 8080]";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Clients connect straight to the agent. None turns direct connections off.
    pub direct: Option<DirectConfig>,
    /// The agent connects out to an orchprox. None turns it off.
    pub orchprox: Option<OrchproxConfig>,
    #[serde(default)]
    pub stream: StreamConfig,
    pub capture: Option<CaptureConfig>,
}

/// Without a settings file, the agent accepts direct connections on the default port.
impl Default for Config {
    fn default() -> Self {
        Self {
            direct: Some(DirectConfig::default()),
            orchprox: None,
            stream: StreamConfig::default(),
            capture: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectConfig {
    pub port: u16,
    pub loopback_only: bool,
    pub bind_address: Option<Ipv4Addr>,
}

impl Default for DirectConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            loopback_only: false,
            bind_address: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchproxConfig {
    /// The orchprox's WebSocket address, e.g. `wss://orchprox.example.com`.
    pub url: String,
}

/// Opt-in capture of a named video source in the Linux PipeWire session.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureConfig {
    pub pipewire_node: String,
    pub width_px: i32,
    pub height_px: i32,
    pub eis_socket: Option<PathBuf>,
    pub pipewire_cursor_socket: Option<PathBuf>,
    pub pipewire_audio_sink: Option<String>,
}

impl CaptureConfig {
    pub fn resolution(&self) -> Resolution {
        Resolution {
            width: self.width_px,
            height: self.height_px,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if !cfg!(target_os = "linux") {
            return Err("[capture] is supported only on Linux.".into());
        }
        if !valid_node_name(&self.pipewire_node) {
            return Err("[capture] pipewire_node must be a non-empty node name.".into());
        }
        if self
            .pipewire_audio_sink
            .as_deref()
            .is_some_and(|name| !valid_sink_name(name))
        {
            return Err(
                "[capture] pipewire_audio_sink must start with a letter or underscore and contain only ASCII letters, digits, dots, underscores, or hyphens."
                    .into(),
            );
        }
        for (name, value) in [("width_px", self.width_px), ("height_px", self.height_px)] {
            if value < 64 || value % 2 != 0 {
                return Err(format!(
                    "[capture] {name} must be an even number of at least 64 pixels."
                ));
            }
        }
        for (name, socket) in [
            ("eis_socket", &self.eis_socket),
            ("pipewire_cursor_socket", &self.pipewire_cursor_socket),
        ] {
            let Some(socket) = socket else { continue };
            let path = socket.to_string_lossy();
            if path.trim().is_empty()
                || path.trim() != path
                || path.chars().any(char::is_control)
                || (!socket.is_absolute() && socket.file_name() != Some(socket.as_os_str()))
            {
                return Err(format!(
                    "[capture] {name} must be an absolute path or socket filename."
                ));
            }
        }
        Ok(())
    }
}

fn valid_node_name(name: &str) -> bool {
    !name.is_empty() && name.trim() == name && !name.chars().any(char::is_control)
}

fn valid_sink_name(name: &str) -> bool {
    // PipeWire also interprets numeric target-object values as object serials.
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

impl Config {
    /// Loads the settings file and applies the command-line arguments over it.
    pub fn from_args(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let args = Args::parse(args)?;
        let mut config = match &args.config {
            Some(path) => Self::read(path)?,
            None => Self::read_default()?,
        };
        // --port turns direct connections on, even if the file leaves them off.
        if let Some(port) = args.port {
            config.direct.get_or_insert_with(DirectConfig::default).port = port;
        }
        config.validate()?;
        Ok(config)
    }

    // agent.toml next to the executable, or the defaults if there is none.
    fn read_default() -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("Could not find the agent's folder: {error}"))?;
        let path = executable.with_file_name(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text, &path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("Could not read {}: {error}", path.display())),
        }
    }

    fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        Self::parse(&text, path)
    }

    fn parse(text: &str, path: &Path) -> Result<Self, String> {
        // toml's message names the line and what's wrong with it.
        toml::from_str(text).map_err(|error| format!("Invalid {}: {error}", path.display()))
    }

    fn validate(&self) -> Result<(), String> {
        if self.direct.is_none() && self.orchprox.is_none() {
            return Err("Turn on [direct], [orchprox] or both in agent.toml.".into());
        }
        if self.direct.as_ref().is_some_and(|direct| direct.port == 0) {
            return Err("The [direct] port must be 1–65535.".into());
        }
        if let Some(orchprox) = &self.orchprox
            && !(orchprox.url.starts_with("ws://") || orchprox.url.starts_with("wss://"))
        {
            return Err(format!(
                "The [orchprox] url must start with ws:// or wss://, not {}.",
                orchprox.url
            ));
        }
        if let Some(capture) = &self.capture {
            capture.validate()?;
        }
        self.stream.validate()
    }
}

#[derive(Debug, Default, PartialEq)]
struct Args {
    config: Option<PathBuf>,
    port: Option<u16>,
}

impl Args {
    /// Reads `--name value` or `--name=value`. Names are case-insensitive, so
    /// `--Port` works too.
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self::default();
        while let Some(arg) = args.next() {
            let (name, value) = match arg.split_once('=') {
                Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
                None => (arg, None),
            };
            let name = name.to_ascii_lowercase();
            if name != "--config" && name != "--port" {
                return Err(format!("Unknown argument {name}. {USAGE}"));
            }
            let value = value
                .or_else(|| args.next())
                .ok_or_else(|| format!("{name} needs a value. {USAGE}"))?;
            if name == "--config" {
                parsed.config = Some(value.into());
            } else {
                let port = value.parse().map_err(|_| format!("Invalid port {value}"))?;
                parsed.port = Some(port);
            }
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, String> {
        let config = Config::parse(text, Path::new(FILE_NAME))?;
        config.validate()?;
        Ok(config)
    }

    fn args(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|arg| arg.to_string()))
    }

    fn stream(text: &str) -> Result<StreamConfig, String> {
        parse(&format!("[direct]\n[stream]\n{text}")).map(|config| config.stream)
    }

    #[test]
    fn reads_every_setting() {
        let config = parse(
            r#"
            [direct]
            port = 9000
            loopback_only = true
            bind_address = "192.168.1.20"

            [orchprox]
            url = "wss://orchprox.example.com"

            [stream]
            max_res_w_px = 1920
            max_res_h_px = 1080
            default_fps = 60
            max_fps = 120
            default_bitrate_kbps = 6000
            max_bitrate_kbps = 25000
            "#,
        )
        .unwrap();
        assert_eq!(
            config,
            Config {
                direct: Some(DirectConfig {
                    port: 9000,
                    loopback_only: true,
                    bind_address: Some(Ipv4Addr::new(192, 168, 1, 20)),
                }),
                orchprox: Some(OrchproxConfig {
                    url: "wss://orchprox.example.com".into()
                }),
                stream: StreamConfig {
                    max_res_w_px: Some(1920),
                    max_res_h_px: Some(1080),
                    default_fps: 60,
                    max_fps: 120,
                    default_bitrate_kbps: 6000,
                    max_bitrate_kbps: 25000,
                },
                capture: None,
            }
        );
    }

    #[test]
    fn the_example_file_is_valid() {
        let config = parse(include_str!("../agent.example.toml")).unwrap();
        assert!(config.direct.is_some());
    }

    #[test]
    fn omitted_settings_use_the_defaults() {
        let config = parse("[direct]").unwrap();
        assert_eq!(config.direct, Some(DirectConfig::default()));
        assert_eq!(config.orchprox, None);
        assert_eq!(config.stream, StreamConfig::default());
        assert_eq!(config.capture, None);
    }

    fn capture(settings: &str) -> Result<Config, String> {
        parse(&format!("[direct]\n[capture]\n{settings}"))
    }

    #[test]
    fn named_capture_is_linux_only() {
        let config = capture("pipewire_node = \"gamescope\"\nwidth_px = 1280\nheight_px = 720");
        if cfg!(target_os = "linux") {
            let capture = config.unwrap().capture.unwrap();
            assert_eq!(capture.pipewire_node, "gamescope");
            assert_eq!(capture.eis_socket, None);
            assert_eq!(capture.pipewire_cursor_socket, None);
            assert_eq!(capture.pipewire_audio_sink, None);
            assert_eq!(
                capture.resolution(),
                Resolution {
                    width: 1280,
                    height: 720
                }
            );
        } else {
            assert_eq!(config.unwrap_err(), "[capture] is supported only on Linux.");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn validates_optional_pipewire_cursor_socket() {
        let config = |socket: &str| {
            capture(&format!(
                "pipewire_node = 'gamescope'\nwidth_px = 1280\nheight_px = 720\npipewire_cursor_socket = '{socket}'"
            ))
        };
        for socket in [
            "containerz-cursor.sock",
            "/run/user/1000/containerz-cursor.sock",
        ] {
            assert_eq!(
                config(socket)
                    .unwrap()
                    .capture
                    .unwrap()
                    .pipewire_cursor_socket
                    .as_deref(),
                Some(Path::new(socket))
            );
        }
        assert!(
            config("../containerz-cursor.sock")
                .unwrap_err()
                .contains("pipewire_cursor_socket")
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn rejects_invalid_named_capture_settings() {
        for settings in [
            "pipewire_node = \" \"\nwidth_px = 1280\nheight_px = 720",
            "pipewire_node = \"gamescope \"\nwidth_px = 1280\nheight_px = 720",
            "pipewire_node = \"gamescope\"\nwidth_px = 1281\nheight_px = 720",
            "pipewire_node = \"gamescope\"\nwidth_px = 1280\nheight_px = 32",
            "pipewire_node = \"gamescope\"\nwidth_px = 1280",
        ] {
            assert!(capture(settings).is_err(), "{settings}");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn validates_eis_socket_paths() {
        let config = |socket: &str| {
            capture(&format!(
                "pipewire_node = 'gamescope'\nwidth_px = 1280\nheight_px = 720\neis_socket = '{socket}'"
            ))
        };
        for socket in ["gamescope-0-ei", "/run/user/1000/gamescope-0-ei"] {
            assert_eq!(
                config(socket)
                    .unwrap()
                    .capture
                    .unwrap()
                    .eis_socket
                    .as_deref(),
                Some(Path::new(socket))
            );
        }
        for socket in [
            "",
            " gamescope-0-ei",
            ".",
            "..",
            "../gamescope-0-ei",
            "./gamescope-0-ei",
            "dir/gamescope-0-ei",
        ] {
            assert!(
                config(socket).unwrap_err().contains("eis_socket"),
                "{socket:?}"
            );
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn validates_optional_pipewire_audio_sink() {
        let config = |sink: &str| {
            capture(&format!(
                "pipewire_node = 'gamescope'\nwidth_px = 1280\nheight_px = 720\npipewire_audio_sink = {sink}"
            ))
        };
        assert_eq!(
            config("'game.audio'")
                .unwrap()
                .capture
                .unwrap()
                .pipewire_audio_sink
                .as_deref(),
            Some("game.audio")
        );
        for sink in [
            "''",
            "' game.audio'",
            "'game.audio '",
            r#""game\n.audio""#,
            "'42'",
            "'0x2a'",
        ] {
            assert!(
                config(sink).unwrap_err().contains("pipewire_audio_sink"),
                "{sink}"
            );
        }
    }

    #[test]
    fn a_missing_section_turns_that_connection_off() {
        let config = parse("[orchprox]\nurl = \"ws://localhost:9000\"").unwrap();
        assert_eq!(config.direct, None);
    }

    #[test]
    fn needs_at_least_one_connection() {
        assert!(parse("").is_err());
        assert!(parse("[stream]\ndefault_fps = 60").is_err());
    }

    #[test]
    fn rejects_unknown_settings() {
        assert!(parse("[direct]\nprot = 9000").is_err());
        assert!(parse("[direct]\n[stream]\nmax_framerate = 60").is_err());
    }

    #[test]
    fn rejects_an_orchprox_url_that_isnt_a_websocket() {
        assert!(parse("[orchprox]\nurl = \"https://orchprox.example.com\"").is_err());
    }

    #[test]
    fn rejects_stream_settings_out_of_range() {
        for text in [
            "max_res_w_px = 63",
            "max_res_h_px = 0",
            "max_fps = 121",
            "max_fps = 30\ndefault_fps = 60",
            "default_fps = 0",
            "max_bitrate_kbps = 50001",
            "max_bitrate_kbps = 4000\ndefault_bitrate_kbps = 6000",
            "default_bitrate_kbps = 499",
        ] {
            assert!(stream(text).is_err(), "{text}");
        }
    }

    #[test]
    fn reads_both_argument_forms_case_insensitively() {
        assert_eq!(
            args(&["--Port", "9000", "--config=other.toml"]),
            Ok(Args {
                config: Some("other.toml".into()),
                port: Some(9000)
            })
        );
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(args(&["--port"]).is_err());
        assert!(args(&["--port", "x"]).is_err());
        assert!(args(&["--verbose"]).is_err());
    }
}
