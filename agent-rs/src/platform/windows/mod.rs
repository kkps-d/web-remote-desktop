//! Windows capture, encoders, display lookup and input injection.

mod input;
mod scan_codes;

pub use input::{InputInjector, use_physical_pixels};

use ::windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
use ::windows::core::PCWSTR;
use gstreamer as gst;
use gstreamer::prelude::*;

use crate::encoders::EncoderDefinition;
use crate::settings::Resolution;

/// Captures the primary monitor on the GPU, ready for the output capsfilter.
pub const CAPTURE: &str = "d3d11screencapturesrc name=capture show-cursor=true ! d3d11convert";

/// Shows or hides the cursor in the capture named "capture" in `CAPTURE`, while streaming.
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

/// The primary display's mode in physical pixels, which d3d11screencapturesrc captures by
/// default; unlike GetSystemMetrics, it isn't scaled for DPI. None if it can't be read.
pub fn primary_display_size() -> Option<Resolution> {
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // SAFETY: mode is a valid DEVMODEW with dmSize set, as the API requires.
    let found = unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut mode) };
    found.as_bool().then_some(Resolution {
        width: mode.dmPelsWidth as i32,
        height: mode.dmPelsHeight as i32,
    })
}
