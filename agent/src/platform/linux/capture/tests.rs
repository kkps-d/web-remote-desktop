use std::sync::mpsc::sync_channel;

use super::*;
use crate::platform::{ENCODERS, output_caps};

#[test]
#[ignore = "requires the GStreamer x264 runtime plugin"]
fn changes_live_caps_with_a_frame_at_videorate_output() {
    gst::init().unwrap();
    for (old_fps, new_fps) in [(30, 15), (15, 60)] {
        for width in [320, 160] {
            check_live_caps(old_fps, new_fps, width);
        }
    }
}

fn check_live_caps(old_fps: i32, new_fps: i32, width: i32) {
    let initial_size = Resolution {
        width: 320,
        height: 240,
    };
    let size = Resolution {
        width,
        height: width * 3 / 4,
    };
    let encoder = ENCODERS
        .iter()
        .find(|encoder| encoder.id == "x264")
        .unwrap();
    let pipeline = gst::parse::launch(&format!(
        "videotestsrc is-live=true ! video/x-raw,width=320,height=240,framerate=144/1 ! \
         {} ! capsfilter name=output caps={} ! {} ! fakesink name=sink sync=false",
        video_processing(old_fps),
        output_caps(Some(initial_size), old_fps),
        (encoder.describe)("x264enc", 2000),
    ))
    .unwrap()
    .downcast::<gst::Pipeline>()
    .unwrap();
    let rate = pipeline.by_name("rate").unwrap();
    let output = pipeline.by_name("output").unwrap();
    let videorate_src = rate.static_pad("sink").unwrap().peer().unwrap();
    videorate_src
        .parent_element()
        .unwrap()
        .static_pad("sink")
        .unwrap()
        .add_probe(gst::PadProbeType::EVENT_UPSTREAM, |_, info| {
            if info.event().unwrap().type_() == gst::EventType::Reconfigure {
                gst::PadProbeReturn::Drop
            } else {
                gst::PadProbeReturn::Ok
            }
        });

    let (encoded, frames) = sync_channel(3);
    let new_framerate = gst::Fraction::new(new_fps, 1);
    let encoded_pad = pipeline
        .by_name("sink")
        .unwrap()
        .static_pad("sink")
        .unwrap();
    encoded_pad.add_probe(gst::PadProbeType::BUFFER, move |pad, _| {
        let caps = pad.current_caps().unwrap();
        let caps = caps.structure(0).unwrap();
        if caps.get::<gst::Fraction>("framerate") == Ok(new_framerate)
            && caps.get::<i32>("width") == Ok(size.width)
            && caps.get::<i32>("height") == Ok(size.height)
        {
            let _ = encoded.try_send(());
        }
        gst::PadProbeReturn::Ok
    });

    let timeout = Duration::from_secs(2);
    pipeline.set_state(gst::State::Playing).unwrap();
    assert_eq!(
        pipeline.state(gst::ClockTime::from_seconds(2)).1,
        gst::State::Playing
    );

    let (held, in_flight) = sync_channel(1);
    let probe = videorate_src
        .add_probe(
            gst::PadProbeType::BLOCK | gst::PadProbeType::BUFFER,
            move |_, _| {
                let _ = held.try_send(());
                gst::PadProbeReturn::Ok
            },
        )
        .unwrap();
    in_flight.recv_timeout(timeout).unwrap();
    // A frame-rate update also rewrites output caps when the resolution stays the same.
    rate.set_property("caps", gst::Caps::from_str(&rate_caps(new_fps)).unwrap());
    output.set_property(
        "caps",
        gst::Caps::from_str(&output_caps(Some(size), new_fps)).unwrap(),
    );
    videorate_src.remove_probe(probe);

    let continued = (0..3).all(|_| frames.recv_timeout(timeout).is_ok());
    let error = pipeline
        .bus()
        .unwrap()
        .timed_pop_filtered(gst::ClockTime::ZERO, &[gst::MessageType::Error]);
    let caps = output.static_pad("src").unwrap().current_caps().unwrap();
    pipeline.set_state(gst::State::Null).unwrap();

    let trial = format!(
        "{old_fps} → {new_fps} fps at {}×{}",
        size.width, size.height
    );
    assert!(error.is_none(), "{trial}: {error:?}");
    assert!(
        continued,
        "{trial}: expected three encoded frames at the new caps"
    );
    let caps = caps.structure(0).unwrap();
    assert_eq!(
        caps.get::<gst::Fraction>("framerate").unwrap(),
        new_framerate
    );
    assert_eq!(caps.get::<i32>("width").unwrap(), size.width);
    assert_eq!(caps.get::<i32>("height").unwrap(), size.height);
}
