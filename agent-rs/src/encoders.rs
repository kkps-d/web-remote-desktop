//! The H.264 encoders from docs/ENCODERS.md and which of them work on this machine. GPU
//! encoders count only on the GPU that drives the captured display; any other GPU would need a
//! copy between GPUs for every frame.

use std::collections::HashMap;

use gstreamer as gst;
use gstreamer::prelude::*;
use regex::Regex;
use serde::Serialize;
use tracing::info;

use crate::platform;
use crate::settings::SizeLimits;

/// An encoder this OS can use, if it is installed.
pub struct EncoderDefinition {
    pub id: &'static str,
    pub label: &'static str,
    /// Matches the element factory names that provide it.
    pub factory_pattern: &'static str,
    /// Whether it encodes on a GPU, i.e. its factory has the Hardware class.
    pub on_gpu: bool,
    /// The encoder bin's description, from the factory name and the bitrate in kbps.
    pub describe: fn(&str, i32) -> String,
}

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

impl EncoderCatalog {
    /// Finds the installed encoders that run on the display's GPU, or on the CPU.
    pub fn detect() -> Result<Self, String> {
        let display = platform::find_display_gpu();
        let installed: Vec<gst::ElementFactory> = gst::Registry::get()
            .features(gst::ElementFactory::static_type())
            .into_iter()
            .filter_map(|feature| feature.downcast().ok())
            .collect();

        let mut available = HashMap::new();
        let mut options = Vec::new();
        for definition in platform::ENCODERS {
            let pattern = Regex::new(definition.factory_pattern).unwrap();
            let mut candidates: Vec<_> = installed
                .iter()
                .filter(|factory| {
                    pattern.is_match(&factory.name()) && is_hardware(factory) == definition.on_gpu
                })
                .collect();
            // The unsuffixed factory (the first adapter) before the deviceN ones.
            candidates.sort_by_key(|factory| (factory.name().len(), factory.name()));
            let found = if definition.on_gpu {
                display.as_ref().and_then(|display| {
                    candidates
                        .iter()
                        .find(|factory| platform::runs_on(factory, display))
                })
            } else {
                candidates.first()
            };

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

            let reason = match (found, &display) {
                (Some(factory), _) => {
                    available.insert(
                        definition.id,
                        Available {
                            factory: factory.name().to_string(),
                            limits: size_limits(factory),
                        },
                    );
                    None
                }
                (None, _) if candidates.is_empty() => Some("Not installed".into()),
                (None, None) => Some("Couldn't find the display's GPU".into()),
                (None, Some(display)) => Some(format!(
                    "Not on this display's GPU ({})",
                    display.description
                )),
            };
            options.push(option(definition, reason));
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
        // Named apart from `display`, which tracing's macros read as a keyword.
        let gpu = display.as_ref().map_or("unknown", |gpu| &gpu.description);
        info!("Display GPU: {gpu}. Encoders: {}", found.join(", "));
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
