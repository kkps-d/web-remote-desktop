//! Reads the codecs the stream must use from the browser's SDP offer.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

/// The browser's offer can't be answered, e.g. it has no usable codec.
#[derive(Debug, PartialEq)]
pub struct InvalidOffer(pub String);

impl fmt::Display for InvalidOffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidOffer {}

#[derive(Debug, Clone, PartialEq)]
pub struct H264Codec {
    pub payload_type: u32,
    pub profile_level_id: String,
}

static H264_RTPMAP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"a=rtpmap:(\d+) H264/90000").unwrap());
static CONSTRAINED_BASELINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("profile-level-id=(42e0[0-9a-f]{2})").unwrap());
// Encoding names are case-insensitive (RFC 4566).
static OPUS_RTPMAP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)a=rtpmap:(\d+) opus/48000/2").unwrap());

/// Finds the browser's constrained-baseline, packetization-mode=1 H.264 codec in the offer.
pub fn find_h264_codec(offer_sdp: &str) -> Result<H264Codec, InvalidOffer> {
    for rtpmap in H264_RTPMAP.captures_iter(offer_sdp) {
        let payload_type = &rtpmap[1];
        // `.` stops at \n but not \r, so the value keeps the CRLF's \r, as in the C# agent.
        let fmtp = Regex::new(&format!("a=fmtp:{payload_type} (.*)")).unwrap();
        let Some(parameters) = fmtp.captures(offer_sdp).map(|fmtp| fmtp[1].to_owned()) else {
            continue;
        };
        let Some(profile_level_id) = CONSTRAINED_BASELINE.captures(&parameters) else {
            continue;
        };
        if parameters.contains("packetization-mode=1")
            && let Ok(payload_type) = payload_type.parse()
        {
            return Ok(H264Codec {
                payload_type,
                profile_level_id: profile_level_id[1].to_owned(),
            });
        }
    }
    Err(InvalidOffer(
        "The offer has no constrained-baseline H.264 codec.".into(),
    ))
}

/// The browser's Opus payload type, or None if the offer has no audio.
pub fn find_opus_payload_type(offer_sdp: &str) -> Option<u32> {
    OPUS_RTPMAP.captures(offer_sdp)?[1].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // SDP lines end in CRLF, which leaves a trailing \r on each matched fmtp value.
    fn offer(header: &[&str], lines: &[&str]) -> String {
        let mut all = vec!["v=0"];
        all.extend_from_slice(header);
        all.extend_from_slice(lines);
        all.push("");
        all.join("\r\n")
    }

    fn video_offer(lines: &[&str]) -> String {
        offer(&["m=video 9 UDP/TLS/RTP/SAVPF 96 102 104 106 108"], lines)
    }

    fn h264(payload_type: u32, profile_level_id: &str) -> Result<H264Codec, InvalidOffer> {
        Ok(H264Codec {
            payload_type,
            profile_level_id: profile_level_id.into(),
        })
    }

    #[test]
    fn picks_the_first_constrained_baseline_codec_with_packetization_mode_1() {
        // Chrome lists baseline and packetization-mode=0 variants before the one we can use.
        let offer = video_offer(&[
            "a=rtpmap:96 VP8/90000",
            "a=rtpmap:102 H264/90000",
            "a=fmtp:102 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f",
            "a=rtpmap:104 H264/90000",
            "a=fmtp:104 level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f",
            "a=rtpmap:106 H264/90000",
            "a=fmtp:106 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f",
            "a=rtpmap:108 H264/90000",
            "a=fmtp:108 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e034",
        ]);

        assert_eq!(find_h264_codec(&offer), h264(106, "42e01f"));
    }

    #[test]
    fn accepts_firefox_parameter_order() {
        let offer = video_offer(&[
            "a=rtpmap:126 H264/90000",
            "a=fmtp:126 profile-level-id=42e01f;level-asymmetry-allowed=1;packetization-mode=1",
            "a=rtpmap:97 H264/90000",
            "a=fmtp:97 profile-level-id=42e01f;level-asymmetry-allowed=1",
        ]);

        assert_eq!(find_h264_codec(&offer), h264(126, "42e01f"));
    }

    #[test]
    fn rejects_offers_without_a_usable_codec() {
        let offers: [&[&str]; 5] = [
            &["a=rtpmap:96 VP8/90000"],
            &[
                "a=rtpmap:102 H264/90000",
                "a=fmtp:102 packetization-mode=1;profile-level-id=42001f",
            ],
            &[
                "a=rtpmap:102 H264/90000",
                "a=fmtp:102 packetization-mode=0;profile-level-id=42e01f",
            ],
            &[
                "a=rtpmap:102 H264/90000",
                "a=fmtp:102 packetization-mode=1;profile-level-id=640c1f",
            ],
            &["a=rtpmap:102 H264/90000"],
        ];
        for lines in offers {
            assert!(find_h264_codec(&video_offer(lines)).is_err(), "{lines:?}");
        }
    }

    #[test]
    fn does_not_read_another_payload_types_parameters() {
        // 10 has no fmtp line; 102's must not be taken for it.
        let offer = video_offer(&[
            "a=rtpmap:10 H264/90000",
            "a=rtpmap:102 VP8/90000",
            "a=fmtp:102 packetization-mode=1;profile-level-id=42e01f",
        ]);

        assert!(find_h264_codec(&offer).is_err());
    }

    fn audio_offer(lines: &[&str]) -> String {
        offer(
            &["m=video 9 UDP/TLS/RTP/SAVPF 106", "a=rtpmap:106 H264/90000"],
            lines,
        )
    }

    #[test]
    fn finds_chromes_opus_payload_type() {
        let offer = audio_offer(&[
            "m=audio 9 UDP/TLS/RTP/SAVPF 111 63 9",
            "a=rtpmap:111 opus/48000/2",
            "a=fmtp:111 minptime=10;useinbandfec=1",
            "a=rtpmap:63 red/48000/2",
            "a=rtpmap:9 G722/8000",
        ]);

        assert_eq!(find_opus_payload_type(&offer), Some(111));
    }

    #[test]
    fn matches_the_encoding_name_in_any_case() {
        let offer = audio_offer(&[
            "m=audio 9 UDP/TLS/RTP/SAVPF 109",
            "a=rtpmap:109 OPUS/48000/2",
        ]);

        assert_eq!(find_opus_payload_type(&offer), Some(109));
    }

    #[test]
    fn returns_none_without_opus() {
        assert_eq!(find_opus_payload_type(&audio_offer(&[])), None);
        let g722 = audio_offer(&["m=audio 9 UDP/TLS/RTP/SAVPF 9", "a=rtpmap:9 G722/8000"]);
        assert_eq!(find_opus_payload_type(&g722), None);
    }
}
