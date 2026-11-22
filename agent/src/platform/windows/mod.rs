//! Windows capture, encoders, display lookup and input injection.

mod input;
mod scan_codes;
mod touch;

pub use input::InputInjector;
pub use touch::TouchInjector;

use ::windows::Wdk::System::SystemServices::RtlGetVersion;
use ::windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
use ::windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use ::windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use ::windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
use ::windows::core::PCWSTR;
use gstreamer as gst;
use gstreamer::prelude::*;

use super::EncoderDefinition;
use crate::config::CaptureConfig;
use crate::stream_settings::Resolution;

/// Captures the primary monitor on the GPU, ready for the output capsfilter.
pub struct Capture;

impl Capture {
    pub fn open(_config: Option<&CaptureConfig>) -> Result<Self, String> {
        Ok(Self)
    }

    /// The capture segment of the pipeline; its source is named "capture". The frame rate is in
    /// the output caps.
    pub fn description(&self, _framerate: i32) -> String {
        "d3d11screencapturesrc name=capture show-cursor=true ! d3d11convert".into()
    }

    /// The output caps carry the frame rate on Windows, so there's nothing else to change.
    pub fn set_framerate(&self, _pipeline: &gst::Pipeline, _framerate: i32) -> Result<(), String> {
        Ok(())
    }

    /// Takes the element named "capture" once the pipeline is built. Nothing to do on Windows.
    pub fn attach(&self, _capture: &gst::Element) {}

    /// The primary display's mode in physical pixels, which d3d11screencapturesrc captures by
    /// default; unlike GetSystemMetrics, it isn't scaled for DPI. None if it can't be read.
    pub fn size(&self) -> Option<Resolution> {
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        // SAFETY: mode is a valid DEVMODEW with dmSize set, as the API requires.
        let found =
            unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut mode) };
        found.as_bool().then_some(Resolution {
            width: mode.dmPelsWidth as i32,
            height: mode.dmPelsHeight as i32,
        })
    }
}

/// Captures what every app plays except the agent itself (process loopback), for the audio
/// branch. Unlike loopback of the default device, it keeps working when the output device
/// switches, at about 40 ms more latency; see docs/spikes/audio.md. Without provide-clock=false the
/// source would be the pipeline clock, which stalls on a device switch and freezes the video too.
pub fn audio_capture() -> String {
    format!(
        "wasapi2src loopback=true loopback-mode=exclude-process-tree loopback-target-pid={} \
         low-latency=true provide-clock=false continue-on-error=true",
        std::process::id()
    )
}

/// Why audio can't stream on this computer, or None if it can.
pub fn audio_unavailable() -> Option<&'static str> {
    // GetVersionEx reports an older version to programs without a compatibility manifest, so
    // ask ntdll.
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: version is a valid OSVERSIONINFOW with its size set, as the API requires.
    let _ = unsafe { RtlGetVersion(&mut version) };
    let build = (
        version.dwMajorVersion,
        version.dwMinorVersion,
        version.dwBuildNumber,
    );
    (build < (10, 0, 20348)).then_some("process loopback needs Windows build 20348 or later")
}

/// Shows or hides the cursor in the capture named "capture", while streaming.
pub fn show_cursor(capture: &gst::Element, visible: bool) {
    capture.set_property("show-cursor", visible);
}

// d3d11convert scales to the output size, and the capture source captures at the output frame
// rate. Changing these caps while playing renegotiates the pipeline; the encoder restarts with
// a keyframe and browsers decode the new size without SDP renegotiation.
pub fn output_caps(size: Option<Resolution>, framerate: i32) -> String {
    let size = size.map_or(String::new(), |size| {
        format!("width={},height={},", size.width, size.height)
    });
    format!("video/x-raw(memory:D3D11Memory),format=NV12,{size}framerate={framerate}/1")
}

// Each describes the encoder segment that sits between the capture's D3D11 NV12 output and
// h264parse, from the factory name and bitrate. The element named "encoder" takes bitrate
// changes. Keyframes every 60 frames let a viewer recover quickly from packet loss.
// In priority order: the first available one is the default.
pub static ENCODERS: &[EncoderDefinition] = &[
    EncoderDefinition {
        id: "nvenc",
        label: "NVIDIA NVENC",
        factory_pattern: r"^nvd3d11h264(device\d+)?enc$",
        on_gpu: true,
        describe: |factory, kbps| {
            format!(
                "{factory} name=encoder tune=ultra-low-latency bframes=0 rc-lookahead=0 gop-size=60 bitrate={kbps}"
            )
        },
    },
    EncoderDefinition {
        id: "amf",
        label: "AMD AMF",
        factory_pattern: r"^amfh264(device\d+)?enc$",
        on_gpu: true,
        describe: |factory, kbps| {
            format!("{factory} name=encoder usage=ultra-low-latency gop-size=60 bitrate={kbps}")
        },
    },
    EncoderDefinition {
        id: "mf",
        label: "Media Foundation",
        factory_pattern: r"^mfh264(device\d+)?enc$",
        on_gpu: true,
        describe: |factory, kbps| {
            format!("{factory} name=encoder low-latency=true gop-size=60 bitrate={kbps}")
        },
    },
    // d3d12upload takes the D3D11 frames without a trip through system memory.
    EncoderDefinition {
        id: "d3d12",
        label: "Direct3D 12",
        factory_pattern: r"^d3d12h264(device\d+)?enc$",
        on_gpu: true,
        describe: |factory, kbps| {
            format!(
                "d3d12upload ! {factory} name=encoder rate-control=cbr gop-size=60 bitrate={kbps}"
            )
        },
    },
    EncoderDefinition {
        id: "x264",
        label: "x264 (CPU)",
        factory_pattern: "^x264enc$",
        on_gpu: false,
        describe: |factory, kbps| {
            format!(
                "d3d11download ! {factory} name=encoder tune=zerolatency speed-preset=ultrafast key-int-max=60 bitrate={kbps}"
            )
        },
    },
    // Microsoft's software encoder registers as an mfh264*enc without the Hardware class.
    EncoderDefinition {
        id: "mf-software",
        label: "Microsoft (CPU)",
        factory_pattern: r"^mfh264(device\d+)?enc$",
        on_gpu: false,
        describe: |factory, kbps| {
            format!(
                "d3d11download ! {factory} name=encoder low-latency=true gop-size=60 bitrate={kbps}"
            )
        },
    },
];

/// The GPU that drives the captured display.
pub struct DisplayGpu {
    luid: i64,
    pub description: String,
}

// d3d11screencapturesrc captures the primary monitor by default; its device properties name
// the adapter that drives it.
pub fn find_display_gpu() -> Option<DisplayGpu> {
    let monitor = gst::DeviceMonitor::new();
    monitor.add_filter(Some("Source/Monitor"), None);
    monitor.start().ok()?;
    let gpu = monitor.devices().into_iter().find_map(|device| {
        let properties = device.properties()?;
        let primary = properties.get::<bool>("device.primary").unwrap_or(false);
        if properties.get::<&str>("device.api").ok()? != "d3d11" || !primary {
            return None;
        }
        Some(DisplayGpu {
            luid: properties.get::<i64>("device.adapter.luid").unwrap_or(0),
            description: properties
                .get::<String>("device.adapter.description")
                .unwrap_or_else(|_| "unknown GPU".into()),
        })
    });
    monitor.stop();
    gpu
}

// GPU encoders register one factory per adapter; an instance's adapter-luid names it.
// Creating an NVENC element here counts as running NVENC, and the Media Foundation instances
// created after it would then break NVENC (see EncoderCatalog::detect), so NVENC is matched by
// vendor instead. With several NVIDIA GPUs that picks the first one's encoder.
pub fn runs_on(factory: &gst::ElementFactory, gpu: &DisplayGpu) -> bool {
    if factory.name().starts_with("nvd3d11") {
        return gpu.description.contains("NVIDIA");
    }
    // The element holds a GPU device until it is dropped.
    let Ok(element) = factory.create().build() else {
        return false;
    };
    element.find_property("adapter-luid").is_some()
        && element.property_value("adapter-luid").get::<i64>() == Ok(gpu.luid)
}

/// Makes the process per-monitor DPI aware, so cursor positions and screen sizes are in
/// physical pixels like the captured display. Call once at startup.
pub fn use_physical_pixels() {
    // SAFETY: takes a predefined context and has no other preconditions. It fails only if the
    // awareness is already set, which then stays as it is.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// The primary display's size for input injection, read on every event. In physical pixels,
/// since the process is per-monitor DPI aware. Cheaper than `Capture::size`, which queries
/// the display driver.
fn display_size() -> (i32, i32) {
    // SAFETY: GetSystemMetrics only reads system settings.
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}
