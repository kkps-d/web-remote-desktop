//! Identifies the capture GPU and matches encoder devices to it through DRM and PCI.

use std::ffi::{CStr, c_char, c_int, c_uint};
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use gstreamer as gst;
use gstreamer::prelude::*;
use libloading::Library;
use rustix::fs::{Dev, major, makedev, minor};
use tracing::debug;

use super::wayland_gpu::gnome_render_device;

pub struct DisplayGpu {
    device: PathBuf,
    pub description: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PciAddress {
    domain: u32,
    bus: u8,
    slot: u8,
    function: u8,
}

impl PciAddress {
    fn parse(address: &str) -> Option<Self> {
        let (domain, rest) = address.split_once(':')?;
        let (bus, rest) = rest.split_once(':')?;
        let (slot, function) = rest.split_once('.')?;
        Some(Self {
            domain: u32::from_str_radix(domain, 16).ok()?,
            bus: u8::from_str_radix(bus, 16).ok()?,
            slot: u8::from_str_radix(slot, 16).ok()?,
            function: u8::from_str_radix(function, 16).ok()?,
        })
    }
}

struct DrmNode {
    number: Dev,
    device: PathBuf,
}

pub fn find_capture_gpu(portal: bool) -> Option<DisplayGpu> {
    let nodes = drm_nodes(Path::new("/sys/class/drm"))?;
    let renderer = portal.then(gnome_render_device).flatten();
    let device = capture_device(&nodes, renderer)?;
    let description = device.file_name()?.to_string_lossy().into_owned();
    debug!(%description, ?renderer, "Resolved the Linux capture GPU");
    Some(DisplayGpu {
        device: device.to_owned(),
        description,
    })
}

fn drm_nodes(directory: &Path) -> Option<Vec<DrmNode>> {
    let mut nodes = Vec::new();
    for entry in fs::read_dir(directory).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_str()?;
        if !["card", "renderD"].iter().any(|prefix| {
            name.strip_prefix(prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        }) {
            continue;
        }
        let path = entry.path();
        let number = fs::read_to_string(path.join("dev")).ok()?;
        let (major, minor) = number.trim().split_once(':')?;
        nodes.push(DrmNode {
            number: makedev(major.parse().ok()?, minor.parse().ok()?),
            device: path.join("device").canonicalize().ok()?,
        });
    }
    Some(nodes)
}

fn capture_device(nodes: &[DrmNode], renderer: Option<Dev>) -> Option<&Path> {
    if let Some(renderer) = renderer {
        return nodes
            .iter()
            .find(|node| node.number == renderer)
            .map(|node| node.device.as_path());
    }
    let first = &nodes.first()?.device;
    nodes
        .iter()
        .all(|node| &node.device == first)
        .then_some(first.as_path())
}

pub fn runs_on(factory: &gst::ElementFactory, gpu: &DisplayGpu) -> bool {
    let Ok(encoder) = factory.create().build() else {
        return false;
    };
    if encoder.find_property("device-path").is_some() {
        return encoder
            .property::<Option<String>>("device-path")
            .and_then(|path| drm_device(Path::new(&path)))
            .is_some_and(|device| device == gpu.device);
    }
    if encoder.find_property("cuda-device-id").is_some() {
        return gpu
            .device
            .file_name()
            .and_then(|name| PciAddress::parse(name.to_str()?))
            .is_some_and(|address| {
                cuda_pci_address(encoder.property("cuda-device-id")) == Some(address)
            });
    }
    false
}

fn drm_device(path: &Path) -> Option<PathBuf> {
    let metadata = path.metadata().ok()?;
    if !metadata.file_type().is_char_device() {
        return None;
    }
    let number = metadata.rdev();
    Path::new("/sys/dev/char")
        .join(format!("{}:{}", major(number), minor(number)))
        .join("device")
        .canonicalize()
        .ok()
}

fn cuda_pci_address(ordinal: u32) -> Option<PciAddress> {
    // SAFETY: These symbols are loaded from the CUDA driver and use its documented C signatures.
    // The writable outputs remain valid for the calls, and the library outlives every symbol.
    unsafe {
        let driver = Library::new("libcuda.so.1").ok()?;
        let init = driver
            .get::<unsafe extern "C" fn(c_uint) -> c_int>(b"cuInit")
            .ok()?;
        let get_device = driver
            .get::<unsafe extern "C" fn(*mut c_int, c_int) -> c_int>(b"cuDeviceGet")
            .ok()?;
        let get_address = driver
            .get::<unsafe extern "C" fn(*mut c_char, c_int, c_int) -> c_int>(b"cuDeviceGetPCIBusId")
            .ok()?;
        let ordinal = c_int::try_from(ordinal).ok()?;
        let mut device = 0;
        let mut address = [0_u8; 32];
        if init(0) != 0
            || get_device(&mut device, ordinal) != 0
            || get_address(address.as_mut_ptr().cast(), address.len() as c_int, device) != 0
        {
            return None;
        }
        PciAddress::parse(CStr::from_bytes_until_nul(&address).ok()?.to_str().ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(minor: u32, device: &str) -> DrmNode {
        DrmNode {
            number: makedev(226, minor),
            device: PathBuf::from(device),
        }
    }

    #[test]
    fn counts_card_and_render_node_as_one_gpu() {
        let nodes = [node(0, "/gpu-a"), node(128, "/gpu-a")];
        assert_eq!(capture_device(&nodes, None), Some(Path::new("/gpu-a")));
    }

    #[test]
    fn no_device_or_ambiguous_devices_are_unknown() {
        assert_eq!(capture_device(&[], None), None);
        let nodes = [node(0, "/gpu-a"), node(128, "/gpu-a"), node(129, "/gpu-b")];
        assert_eq!(capture_device(&nodes, None), None);
    }

    #[test]
    fn feedback_selects_its_exact_gpu_and_never_falls_back_to_another() {
        let nodes = [node(0, "/gpu-a"), node(128, "/gpu-a"), node(129, "/gpu-b")];
        assert_eq!(
            capture_device(&nodes, Some(makedev(226, 0))),
            Some(Path::new("/gpu-a"))
        );
        assert_eq!(
            capture_device(&nodes, Some(makedev(226, 129))),
            Some(Path::new("/gpu-b"))
        );
        assert_eq!(capture_device(&nodes[..1], Some(makedev(226, 129))), None);
    }

    #[test]
    fn compares_cuda_and_drm_pci_addresses_without_relying_on_the_vendor() {
        assert_eq!(
            PciAddress::parse("00000000:01:00.0"),
            PciAddress::parse("0000:01:00.0")
        );
        assert_ne!(
            PciAddress::parse("0000:01:00.0"),
            PciAddress::parse("0000:02:00.0")
        );
        assert_ne!(
            PciAddress::parse("0000:01:00.0"),
            PciAddress::parse("0001:01:00.0")
        );
        assert_eq!(PciAddress::parse("renderD128"), None);
    }
}
