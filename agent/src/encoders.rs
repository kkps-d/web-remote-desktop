//! Discovers H.264 encoder factories and selects the ones the capture can use.

use std::collections::HashMap;
use std::sync::Arc;

use gstreamer as gst;
use gstreamer::prelude::*;
use regex::Regex;
use serde::Serialize;
#[cfg(target_os = "linux")]
use tracing::debug;
use tracing::info;

use crate::platform::{self, EncoderDefinition};
use crate::stream_settings::{SizeLimits, StreamConfig};
#[cfg(target_os = "linux")]
use crate::{pipeline::probe_encoder, stream_settings::initial_size};

/// An encoder the viewer can pick, and why it can't be used if it isn't available.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderOption {
    pub id: &'static str,
    pub label: &'static str,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

struct Available {
    factory: String,
    limits: SizeLimits,
}

pub struct EncoderCatalog {
    available: HashMap<&'static str, Available>,
    options: Vec<EncoderOption>,
    default: &'static str,
}

struct EncoderCandidates {
    definition: &'static EncoderDefinition,
    factories: Vec<gst::ElementFactory>,
}

pub struct EncoderInventory {
    #[cfg(windows)]
    catalog: Arc<EncoderCatalog>,
    #[cfg(target_os = "linux")]
    candidates: Vec<EncoderCandidates>,
}

impl EncoderInventory {
    /// Reads the GStreamer registry once at startup.
    pub fn discover() -> Result<Self, String> {
        #[cfg(windows)]
        let gpu = platform::find_display_gpu();

        let installed: Vec<gst::ElementFactory> = gst::Registry::get()
            .features(gst::ElementFactory::static_type())
            .into_iter()
            .filter_map(|feature| feature.downcast().ok())
            .collect();

        let mut candidates = Vec::new();
        for definition in platform::ENCODERS {
            let pattern = Regex::new(definition.factory_pattern).unwrap();
            let mut factories: Vec<_> = installed
                .iter()
                .filter(|factory| {
                    pattern.is_match(&factory.name()) && is_hardware(factory) == definition.on_gpu
                })
                .cloned()
                .collect();
            // The unsuffixed factory (the first adapter) before the deviceN ones.
            factories.sort_by_key(|factory| (factory.name().len(), factory.name()));
            candidates.push(EncoderCandidates {
                definition,
                factories,
            });
        }

        #[cfg(windows)]
        {
            let catalog = EncoderCatalog::select(&candidates, gpu.as_ref(), |_, _, _| Ok(()))?;
            Ok(Self {
                catalog: Arc::new(catalog),
            })
        }

        #[cfg(target_os = "linux")]
        {
            if !candidates
                .iter()
                .any(|group| !group.definition.on_gpu && !group.factories.is_empty())
            {
                return Err(
                    "No x264 encoder is available. Install the GStreamer x264 plugin.".into(),
                );
            }
            let installed = candidates
                .iter()
                .flat_map(|group| {
                    group
                        .factories
                        .iter()
                        .map(|factory| factory.name().to_string())
                })
                .collect::<Vec<_>>();
            info!(
                "Installed H.264 encoder factories: {}",
                installed.join(", ")
            );
            Ok(Self { candidates })
        }
    }

    #[cfg(windows)]
    pub fn for_capture(
        &self,
        _: &platform::Capture,
        _: &StreamConfig,
    ) -> Result<Arc<EncoderCatalog>, String> {
        Ok(self.catalog.clone())
    }

    #[cfg(target_os = "linux")]
    pub fn for_capture(
        &self,
        capture: &platform::Capture,
        config: &StreamConfig,
    ) -> Result<Arc<EncoderCatalog>, String> {
        let gpu = capture.gpu();
        EncoderCatalog::select(
            &self.candidates,
            gpu.as_ref(),
            |definition, factory, limits| {
                let source = capture
                    .size()
                    .ok_or("Couldn't verify the encoder because the capture size is unknown")?;
                let size = initial_size(limits, config.max_size(), Some(source)).unwrap_or(source);
                let description =
                    (definition.describe)(factory.name().as_str(), config.default_bitrate_kbps);
                probe_encoder(&description, size, config.default_fps).map_err(|error| {
                debug!(factory = %factory.name(), %error, "Hardware encoder startup check failed");
                "The encoder failed its startup check".into()
            })
            },
        )
        .map(Arc::new)
    }
}

impl EncoderCatalog {
    fn select(
        candidates: &[EncoderCandidates],
        gpu: Option<&platform::DisplayGpu>,
        verify: impl Fn(&EncoderDefinition, &gst::ElementFactory, SizeLimits) -> Result<(), String>,
    ) -> Result<Self, String> {
        Self::select_with(
            candidates,
            gpu.map(|gpu| gpu.description.as_str()),
            |definition, factory, limits| {
                let gpu = gpu?;
                platform::runs_on(factory, gpu).then(|| verify(definition, factory, limits))
            },
        )
    }

    // For a hardware factory, a missing check result means it didn't match the capture GPU.
    fn select_with(
        candidates: &[EncoderCandidates],
        gpu: Option<&str>,
        check: impl Fn(
            &EncoderDefinition,
            &gst::ElementFactory,
            SizeLimits,
        ) -> Option<Result<(), String>>,
    ) -> Result<Self, String> {
        let mut available = HashMap::new();
        let mut options = Vec::new();
        for &EncoderCandidates {
            definition,
            ref factories,
        } in candidates
        {
            // Once NVENC has run, running any Media Foundation encoder (hardware or software)
            // makes every later NVENC session in this process fail with
            // NV_ENC_ERR_INVALID_VERSION, so they're only offered when NVENC isn't.
            if definition.id.starts_with("mf") && available.contains_key("nvenc") {
                options.push(option(
                    definition,
                    Some("Off because it breaks NVENC until the agent restarts".into()),
                ));
                continue;
            }

            if factories.is_empty() {
                options.push(option(definition, Some("Not installed".into())));
                continue;
            }
            let mut failure = None;
            for factory in factories {
                let limits = size_limits(factory);
                if definition.on_gpu {
                    match check(definition, factory, limits) {
                        Some(Ok(())) => (),
                        Some(Err(reason)) => {
                            failure = Some(reason);
                            continue;
                        }
                        None => continue,
                    }
                }
                available.insert(
                    definition.id,
                    Available {
                        factory: factory.name().to_string(),
                        limits,
                    },
                );
                break;
            }
            if available.contains_key(definition.id) {
                options.push(option(definition, None));
            } else {
                let reason = failure.unwrap_or_else(|| match gpu {
                    Some(gpu) => format!("No encoder matched the capture GPU ({gpu})"),
                    None => "Couldn't identify the capture GPU".into(),
                });
                options.push(option(definition, Some(reason)));
            }
        }

        let default = options
            .iter()
            .find(|option| option.available)
            .map(|option| option.id)
            .ok_or(
                "No H.264 encoder is available. Install GStreamer with the \"Complete\" option.",
            )?;
        let found: Vec<_> = options
            .iter()
            .filter_map(|option| {
                let encoder = available.get(option.id)?;
                Some(format!(
                    "{} ({}, {})",
                    option.id, encoder.factory, encoder.limits
                ))
            })
            .collect();
        let gpu = gpu.unwrap_or("unknown");
        info!("Capture GPU: {gpu}. Encoders: {}", found.join(", "));
        Ok(Self {
            available,
            options,
            default,
        })
    }

    pub fn options(&self) -> &[EncoderOption] {
        &self.options
    }

    /// The highest-priority available encoder.
    pub fn default(&self) -> &'static str {
        self.default
    }

    pub fn is_available(&self, id: &str) -> bool {
        self.available.contains_key(id)
    }

    pub fn label(&self, id: &str) -> &'static str {
        definition(id).label
    }

    /// The frame sizes an available encoder accepts.
    pub fn limits(&self, id: &str) -> SizeLimits {
        self.available[id].limits
    }

    /// The encoder segment for `gst::parse::bin_from_description`.
    pub fn describe(&self, id: &str, bitrate_kbps: i32) -> String {
        (definition(id).describe)(&self.available[id].factory, bitrate_kbps)
    }
}

fn definition(id: &str) -> &'static EncoderDefinition {
    platform::ENCODERS
        .iter()
        .find(|definition| definition.id == id)
        .unwrap()
}

fn option(definition: &EncoderDefinition, unavailable_reason: Option<String>) -> EncoderOption {
    EncoderOption {
        id: definition.id,
        label: definition.label,
        available: unavailable_reason.is_none(),
        unavailable_reason,
    }
}

fn is_hardware(factory: &gst::ElementFactory) -> bool {
    factory.klass().contains("Hardware")
}

// Read from the factory rather than an instance; see platform::runs_on. Hardware encoders
// report their GPU's limits here. Every sink structure (GPU or system memory) has the same ranges.
fn size_limits(factory: &gst::ElementFactory) -> SizeLimits {
    let caps = factory
        .static_pad_templates()
        .into_iter()
        .find(|template| template.direction() == gst::PadDirection::Sink)
        .map(|template| template.caps());
    let structure = caps.as_ref().and_then(|caps| caps.structure(0));
    let range = |field| {
        structure
            .and_then(|structure| structure.get::<gst::IntRange<i32>>(field).ok())
            .map_or((1, i32::MAX), |range| (range.min(), range.max()))
    };
    let (min_width, max_width) = range("width");
    let (min_height, max_height) = range("height");
    SizeLimits {
        min_width,
        max_width,
        min_height,
        max_height,
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn candidates() -> Vec<EncoderCandidates> {
        gst::init().unwrap();
        let factory = gst::ElementFactory::find("fakesink").unwrap();
        platform::ENCODERS
            .iter()
            .map(|definition| EncoderCandidates {
                definition,
                factories: vec![factory.clone()],
            })
            .collect()
    }

    #[test]
    fn prefers_nvenc_when_both_hardware_encoders_pass() {
        let catalog =
            EncoderCatalog::select_with(&candidates(), Some("gpu"), |_, _, _| Some(Ok(())))
                .unwrap();
        assert_eq!(catalog.default(), "nvenc");
        assert!(catalog.is_available("va"));
        assert!(catalog.is_available("x264"));
    }

    #[test]
    fn tries_va_after_nvenc_fails_and_uses_x264_when_both_fail() {
        for va_passes in [true, false] {
            let catalog =
                EncoderCatalog::select_with(&candidates(), Some("gpu"), |definition, _, _| {
                    Some(if definition.id == "va" && va_passes {
                        Ok(())
                    } else {
                        Err("Startup check failed".into())
                    })
                })
                .unwrap();
            assert_eq!(catalog.default(), if va_passes { "va" } else { "x264" });
            assert!(!catalog.is_available("nvenc"));
            assert_eq!(
                catalog.options()[0].unavailable_reason.as_deref(),
                Some("Startup check failed")
            );
        }
    }

    #[test]
    fn explains_unknown_gpu_and_wrong_device() {
        let candidates = candidates();
        let unknown = EncoderCatalog::select(&candidates, None, |_, _, _| unreachable!()).unwrap();
        assert_eq!(unknown.default(), "x264");
        assert_eq!(
            unknown.options()[0].unavailable_reason.as_deref(),
            Some("Couldn't identify the capture GPU")
        );

        let wrong = EncoderCatalog::select_with(&candidates, Some("gpu"), |_, _, _| None).unwrap();
        assert_eq!(wrong.default(), "x264");
        assert_eq!(
            wrong.options()[0].unavailable_reason.as_deref(),
            Some("No encoder matched the capture GPU (gpu)")
        );
    }
}
