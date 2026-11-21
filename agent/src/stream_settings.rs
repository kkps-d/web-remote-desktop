//! Stream settings: the agent's defaults and limits (`[stream]` in agent.toml), the frame sizes
//! encoders accept, and checking a settings change.

use std::fmt;

use serde::{Deserialize, Serialize};

// The widest ranges a settings file may allow; see StreamConfig for this agent's own limits.
const MAX_FRAMERATE: i32 = 120;
const MIN_BITRATE_KBPS: i32 = 500;
const MAX_BITRATE_KBPS: i32 = 50_000;
const MIN_DIMENSION: i32 = 64;

/// Requested stream settings are out of range.
#[derive(Debug, PartialEq)]
pub struct InvalidSettings(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Resolution {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSettings {
    pub width: i32,
    pub height: i32,
    pub framerate: i32,
    pub bitrate_kbps: i32,
    pub encoder: String,
}

impl StreamSettings {
    pub fn size(&self) -> Resolution {
        Resolution {
            width: self.width,
            height: self.height,
        }
    }
}

/// A partial settings change; missing fields keep their current value.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSettingsUpdate {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub framerate: Option<i32>,
    pub bitrate_kbps: Option<i32>,
    pub encoder: Option<String>,
}

/// The frame sizes an encoder accepts, from its sink pad template.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeLimits {
    pub min_width: i32,
    pub max_width: i32,
    pub min_height: i32,
    pub max_height: i32,
}

impl SizeLimits {
    pub fn fits(&self, size: Resolution) -> bool {
        (self.min_width..=self.max_width).contains(&size.width)
            && (self.min_height..=self.max_height).contains(&size.height)
    }

    /// The size scaled down, keeping its aspect ratio, until it is no larger than the maximum.
    pub fn shrink_to_fit(&self, size: Resolution) -> Resolution {
        let scale = (self.max_width as f64 / size.width as f64)
            .min(self.max_height as f64 / size.height as f64)
            .min(1.0);
        // The encoders need even dimensions.
        Resolution {
            width: (size.width as f64 * scale) as i32 / 2 * 2,
            height: (size.height as f64 * scale) as i32 / 2 * 2,
        }
    }
}

impl fmt::Display for SizeLimits {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}×{} to {}×{}",
            self.min_width, self.min_height, self.max_width, self.max_height
        )
    }
}

/// The stream's settings when a viewer connects, and the most a viewer may change them to.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StreamConfig {
    /// A bounding box for the stream; None means no limit beyond the display's resolution.
    pub max_res_w_px: Option<i32>,
    pub max_res_h_px: Option<i32>,
    pub default_fps: i32,
    pub max_fps: i32,
    pub default_bitrate_kbps: i32,
    pub max_bitrate_kbps: i32,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_res_w_px: None,
            max_res_h_px: None,
            default_fps: 30,
            max_fps: MAX_FRAMERATE,
            default_bitrate_kbps: 8000,
            max_bitrate_kbps: MAX_BITRATE_KBPS,
        }
    }
}

impl StreamConfig {
    /// The bounding box streams must fit in.
    pub fn max_size(&self) -> Resolution {
        Resolution {
            width: self.max_res_w_px.unwrap_or(i32::MAX),
            height: self.max_res_h_px.unwrap_or(i32::MAX),
        }
    }

    /// The largest stream size allowed for the source: the source itself, or scaled down to fit
    /// the bounding box with the same aspect ratio.
    pub fn largest_size(&self, source: Resolution) -> Resolution {
        let max = self.max_size();
        let limits = SizeLimits {
            min_width: 0,
            max_width: max.width,
            min_height: 0,
            max_height: max.height,
        };
        if limits.fits(source) {
            source
        } else {
            limits.shrink_to_fit(source)
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("max_res_w_px", self.max_res_w_px),
            ("max_res_h_px", self.max_res_h_px),
        ] {
            if value.is_some_and(|value| value < MIN_DIMENSION) {
                return Err(format!("{name} must be at least {MIN_DIMENSION}."));
            }
        }
        if !(1..=MAX_FRAMERATE).contains(&self.max_fps) {
            return Err(format!("max_fps must be 1–{MAX_FRAMERATE}."));
        }
        if !(1..=self.max_fps).contains(&self.default_fps) {
            return Err(format!("default_fps must be 1–{} (max_fps).", self.max_fps));
        }
        if !(MIN_BITRATE_KBPS..=MAX_BITRATE_KBPS).contains(&self.max_bitrate_kbps) {
            return Err(format!(
                "max_bitrate_kbps must be {MIN_BITRATE_KBPS}–{MAX_BITRATE_KBPS}."
            ));
        }
        if !(MIN_BITRATE_KBPS..=self.max_bitrate_kbps).contains(&self.default_bitrate_kbps) {
            return Err(format!(
                "default_bitrate_kbps must be {MIN_BITRATE_KBPS}–{} (max_bitrate_kbps).",
                self.max_bitrate_kbps
            ));
        }
        Ok(())
    }
}

// Streams start at native resolution (None), unless the display is larger than the encoder
// accepts or the agent's `max` allows; then they start scaled down to fit, e.g. NVENC's 4096 px
// on a 5K display. A display of unknown size starts native.
pub fn initial_size(
    limits: SizeLimits,
    max: Resolution,
    display: Option<Resolution>,
) -> Option<Resolution> {
    let display = display?;
    let limits = SizeLimits {
        max_width: limits.max_width.min(max.width),
        max_height: limits.max_height.min(max.height),
        ..limits
    };
    (!limits.fits(display)).then(|| limits.shrink_to_fit(display))
}

/// Checked new settings, and whether to switch the encoder before resizing.
#[derive(Debug, PartialEq)]
pub struct SettingsPlan {
    pub next: StreamSettings,
    pub switch_encoder_first: bool,
}

/// Merges an update into the current settings and checks that it can be applied within this
/// agent's limits. `limits_of` returns None for an encoder that isn't available.
pub fn plan_update(
    current: &StreamSettings,
    update: &StreamSettingsUpdate,
    source: Resolution,
    config: &StreamConfig,
    limits_of: impl Fn(&str) -> Option<SizeLimits>,
    label_of: impl Fn(&str) -> String,
) -> Result<SettingsPlan, InvalidSettings> {
    let invalid = |message: String| Err(InvalidSettings(message));
    if update.width.is_some() != update.height.is_some() {
        return invalid("Width and height must be set together.".into());
    }

    let next = StreamSettings {
        width: update.width.unwrap_or(current.width),
        height: update.height.unwrap_or(current.height),
        framerate: update.framerate.unwrap_or(current.framerate),
        bitrate_kbps: update.bitrate_kbps.unwrap_or(current.bitrate_kbps),
        encoder: update
            .encoder
            .clone()
            .unwrap_or_else(|| current.encoder.clone()),
    };

    // The encoder needs even dimensions; upscaling would only cost bandwidth.
    if next.width < MIN_DIMENSION
        || next.height < MIN_DIMENSION
        || next.width > source.width
        || next.height > source.height
        || next.width % 2 != 0
        || next.height % 2 != 0
    {
        return invalid(format!(
            "Resolution must be even, at least {MIN_DIMENSION} px and no larger than {}×{}.",
            source.width, source.height
        ));
    }
    let max = config.max_size();
    if next.width > max.width || next.height > max.height {
        let largest = config.largest_size(source);
        return invalid(format!(
            "This agent allows up to {}×{}.",
            largest.width, largest.height
        ));
    }
    if !(1..=config.max_fps).contains(&next.framerate) {
        return invalid(format!("Frame rate must be 1–{} fps.", config.max_fps));
    }
    if !(MIN_BITRATE_KBPS..=config.max_bitrate_kbps).contains(&next.bitrate_kbps) {
        return invalid(format!(
            "Bitrate must be {MIN_BITRATE_KBPS}–{} kbps.",
            config.max_bitrate_kbps
        ));
    }
    let Some(limits) = limits_of(&next.encoder) else {
        return invalid(format!(
            "The {} encoder isn't available on this computer.",
            next.encoder
        ));
    };

    // An encoder given a size outside its limits fails and ends the session, so check first.
    if !limits.fits(next.size()) {
        return invalid(format!("{} supports {limits}.", label_of(&next.encoder)));
    }
    if next == *current {
        return Ok(SettingsPlan {
            next,
            switch_encoder_first: false,
        });
    }

    // Every step must leave the running encoder with a size it accepts: switch encoder first
    // if the new one takes the current size, otherwise resize first.
    let encoder_changed = next.encoder != current.encoder;
    let switch_encoder_first = encoder_changed && limits.fits(current.size());
    if encoder_changed
        && !switch_encoder_first
        && !limits_of(&current.encoder)
            .is_some_and(|current_limits| current_limits.fits(next.size()))
    {
        return invalid("Change the resolution and the encoder separately.".into());
    }
    Ok(SettingsPlan {
        next,
        switch_encoder_first,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: i32, height: i32) -> Resolution {
        Resolution { width, height }
    }

    fn limits(min_width: i32, max_width: i32, min_height: i32, max_height: i32) -> SizeLimits {
        SizeLimits {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    // NVENC tops out at 4096 px; x264 takes any size.
    fn encoder_limits(id: &str) -> Option<SizeLimits> {
        match id {
            "nvenc" => Some(limits(145, 4096, 49, 4096)),
            "x264" => Some(limits(16, 16384, 16, 16384)),
            "large-only" => Some(limits(3000, 8192, 1600, 8192)),
            _ => None,
        }
    }

    const SOURCE: Resolution = Resolution {
        width: 5120,
        height: 2880,
    };

    fn current() -> StreamSettings {
        StreamSettings {
            width: 4096,
            height: 2304,
            framerate: 30,
            bitrate_kbps: 8000,
            encoder: "nvenc".into(),
        }
    }

    fn plan_from(
        current: &StreamSettings,
        update: StreamSettingsUpdate,
    ) -> Result<SettingsPlan, InvalidSettings> {
        plan_update(
            current,
            &update,
            SOURCE,
            &StreamConfig::default(),
            encoder_limits,
            |id| format!("Label {id}"),
        )
    }

    fn plan(update: StreamSettingsUpdate) -> Result<SettingsPlan, InvalidSettings> {
        plan_from(&current(), update)
    }

    fn resize(width: i32, height: i32) -> StreamSettingsUpdate {
        StreamSettingsUpdate {
            width: Some(width),
            height: Some(height),
            ..Default::default()
        }
    }

    fn with_encoder(update: StreamSettingsUpdate, encoder: &str) -> StreamSettingsUpdate {
        StreamSettingsUpdate {
            encoder: Some(encoder.into()),
            ..update
        }
    }

    fn rates(framerate: i32, bitrate_kbps: i32) -> StreamSettingsUpdate {
        StreamSettingsUpdate {
            framerate: Some(framerate),
            bitrate_kbps: Some(bitrate_kbps),
            ..Default::default()
        }
    }

    fn error_message(result: Result<SettingsPlan, InvalidSettings>) -> String {
        result.expect_err("the update should be rejected").0
    }

    #[test]
    fn omitted_fields_keep_their_value() {
        let update = StreamSettingsUpdate {
            framerate: Some(60),
            ..Default::default()
        };
        assert_eq!(
            plan(update).unwrap().next,
            StreamSettings {
                framerate: 60,
                ..current()
            }
        );
    }

    #[test]
    fn an_empty_update_changes_nothing() {
        assert_eq!(
            plan(Default::default()),
            Ok(SettingsPlan {
                next: current(),
                switch_encoder_first: false
            })
        );
    }

    #[test]
    fn width_and_height_must_be_set_together() {
        assert!(
            plan(StreamSettingsUpdate {
                width: Some(1920),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            plan(StreamSettingsUpdate {
                height: Some(1080),
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn rejects_resolutions_the_stream_cant_use() {
        // Odd, below 64 px, and larger than the source.
        for (width, height) in [
            (1921, 1080),
            (1920, 1081),
            (62, 1080),
            (5122, 2880),
            (5120, 2882),
        ] {
            assert!(
                plan(with_encoder(resize(width, height), "x264")).is_err(),
                "{width}×{height}"
            );
        }
    }

    #[test]
    fn rejects_frame_rates_and_bitrates_out_of_range() {
        for (framerate, bitrate_kbps) in [(0, 8000), (121, 8000), (30, 499), (30, 50_001)] {
            assert!(
                plan(rates(framerate, bitrate_kbps)).is_err(),
                "{framerate} fps, {bitrate_kbps} kbps"
            );
        }
    }

    #[test]
    fn accepts_the_ends_of_each_range() {
        for (framerate, bitrate_kbps) in [(1, 500), (120, 50_000)] {
            assert_eq!(
                plan(rates(framerate, bitrate_kbps)).unwrap().next,
                StreamSettings {
                    framerate,
                    bitrate_kbps,
                    ..current()
                }
            );
        }
    }

    #[test]
    fn rejects_settings_above_the_agents_limits() {
        let config = StreamConfig {
            max_res_w_px: Some(1920),
            max_res_h_px: Some(1080),
            max_fps: 60,
            max_bitrate_kbps: 25_000,
            ..Default::default()
        };
        let plan = |update| {
            plan_update(&current(), &update, SOURCE, &config, encoder_limits, |id| {
                id.into()
            })
        };

        assert_eq!(
            error_message(plan(resize(2560, 1440))),
            "This agent allows up to 1920×1080."
        );
        assert!(plan(rates(61, 8000)).is_err());
        assert!(plan(rates(30, 25_001)).is_err());
        assert!(
            plan(StreamSettingsUpdate {
                framerate: Some(60),
                bitrate_kbps: Some(25_000),
                ..resize(1920, 1080)
            })
            .is_ok()
        );
    }

    #[test]
    fn rejects_an_unavailable_encoder() {
        assert!(plan(with_encoder(Default::default(), "amf")).is_err());
    }

    #[test]
    fn rejects_a_size_outside_the_encoders_limits_naming_it() {
        assert_eq!(
            error_message(plan(resize(5120, 2880))),
            "Label nvenc supports 145×49 to 4096×4096."
        );
    }

    #[test]
    fn switches_first_when_the_new_encoder_takes_the_current_size() {
        // NVENC can't take 5120 px, so x264 must be running before the resize.
        let plan = plan(with_encoder(resize(5120, 2880), "x264")).unwrap();
        assert_eq!(
            plan.next,
            StreamSettings {
                width: 5120,
                height: 2880,
                framerate: 30,
                bitrate_kbps: 8000,
                encoder: "x264".into()
            }
        );
        assert!(plan.switch_encoder_first);
    }

    #[test]
    fn resizes_first_when_only_the_current_encoder_takes_both_sizes() {
        // NVENC can't take x264's current 5120 px, so shrink before switching.
        let current = StreamSettings {
            width: 5120,
            height: 2880,
            encoder: "x264".into(),
            ..current()
        };
        let plan = plan_from(&current, with_encoder(resize(4096, 2304), "nvenc")).unwrap();
        assert!(!plan.switch_encoder_first);
    }

    #[test]
    fn rejects_a_change_no_order_can_apply() {
        // Neither NVENC at 4200 px wide nor "large-only" at 1920 px works.
        let current = StreamSettings {
            width: 1920,
            height: 1080,
            ..current()
        };
        let result = plan_from(&current, with_encoder(resize(4200, 2400), "large-only"));
        assert_eq!(
            error_message(result),
            "Change the resolution and the encoder separately."
        );
    }

    const NVENC: SizeLimits = SizeLimits {
        min_width: 145,
        max_width: 4096,
        min_height: 49,
        max_height: 4096,
    };

    #[test]
    fn fits_includes_both_ends() {
        let cases = [
            (145, 49, true),
            (4096, 4096, true),
            (144, 1080, false),
            (1920, 48, false),
            (4097, 1080, false),
            (1920, 4097, false),
        ];
        for (width, height, fits) in cases {
            assert_eq!(NVENC.fits(size(width, height)), fits, "{width}×{height}");
        }
    }

    #[test]
    fn shrinks_a_5k_display_to_the_limit_keeping_its_aspect_ratio() {
        assert_eq!(NVENC.shrink_to_fit(size(5120, 2880)), size(4096, 2304));
    }

    #[test]
    fn shrinks_to_even_dimensions() {
        let shrunk = NVENC.shrink_to_fit(size(5000, 3001));

        assert!(NVENC.fits(shrunk));
        assert_eq!(shrunk.width % 2, 0);
        assert_eq!(shrunk.height % 2, 0);
        // Equal to 2 decimal places.
        let round = |ratio: f64| (ratio * 100.0).round();
        let aspect = shrunk.width as f64 / shrunk.height as f64;
        assert_eq!(round(aspect), round(5000.0 / 3001.0), "{aspect}");
    }

    #[test]
    fn leaves_a_size_that_fits_alone() {
        assert_eq!(NVENC.shrink_to_fit(size(1920, 1080)), size(1920, 1080));
    }

    #[test]
    fn the_largest_size_fits_the_box_keeping_the_aspect_ratio() {
        let config = StreamConfig {
            max_res_w_px: Some(1920),
            max_res_h_px: Some(1080),
            ..Default::default()
        };
        assert_eq!(config.largest_size(size(2560, 1440)), size(1920, 1080));
        assert_eq!(config.largest_size(size(3440, 1440)), size(1920, 802));
        assert_eq!(config.largest_size(size(1280, 720)), size(1280, 720));

        let width_only = StreamConfig {
            max_res_w_px: Some(1280),
            ..Default::default()
        };
        assert_eq!(width_only.largest_size(size(2560, 1440)), size(1280, 720));
    }

    const NO_MAX: Resolution = Resolution {
        width: i32::MAX,
        height: i32::MAX,
    };

    #[test]
    fn starts_native_when_the_display_fits() {
        assert_eq!(initial_size(NVENC, NO_MAX, Some(size(2560, 1440))), None);
        assert_eq!(initial_size(NVENC, NO_MAX, None), None);
    }

    #[test]
    fn starts_scaled_to_the_encoders_limits() {
        assert_eq!(
            initial_size(NVENC, NO_MAX, Some(size(5120, 2880))),
            Some(size(4096, 2304))
        );
    }

    #[test]
    fn starts_scaled_to_the_agents_maximum() {
        assert_eq!(
            initial_size(NVENC, size(1920, 1080), Some(size(2560, 1440))),
            Some(size(1920, 1080))
        );
    }
}
