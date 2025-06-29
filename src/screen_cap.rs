use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use color_eyre::eyre::{ContextCompat, bail};
use gstreamer::prelude::*;
use gstreamer::{self as gst, MessageView};
use gstreamer_app as gst_app;
use log::{debug, error, trace, warn};
use tokio::sync::watch::Sender;

pub fn start_pipeline(tx: Sender<Vec<u8>>) -> color_eyre::Result<()> {
    let source = gst::ElementFactory::make("d3d11screencapturesrc")
        .name("source")
        .property_from_str("capture-api", "0")
        .property_from_str("monitor-index", "1") // TODO: Dectect bogus index
        .property_from_str("show-cursor", "true")
        .build()?;
    let capsfilter = gst::ElementFactory::make_with_name("capsfilter", Some("rate_filter"))?;
    let caps = gst::Caps::builder("video/x-raw")
        .features(["memory:D3D11Memory"])
        .field("framerate", gst::Fraction::new(60, 1))
        .build();
    capsfilter.set_property("caps", &caps);
    let queue = gst::ElementFactory::make("queue")
        .name("queue")
        .property_from_str("max-size-buffers", "1")
        .property_from_str("max-size-time", "0")
        .property_from_str("max-size-bytes", "0")
        .property_from_str("leaky", "2")
        .build()?;
    let convert = gst::ElementFactory::make_with_name("d3d11convert", Some("convert"))?;
    let encoder = gst::ElementFactory::make("nvd3d11h264enc")
        .name("encoder")
        .property_from_str("aq-strength", "3")
        .property_from_str("spatial-aq", "true")
        .property_from_str("temporal-aq", "true")
        .property_from_str("zerolatency", "true")
        .property_from_str("repeat-sequence-header", "true")
        .property_from_str("bitrate", "0")
        .property_from_str("max-bitrate", "20000")
        .property_from_str("vbv-buffer-size", "20000")
        .property_from_str("gop-size", "10")
        .property_from_str("rc-mode", "2")
        .property_from_str("preset", "8")
        .property_from_str("tune", "3")
        .build()?;
    let parser = gst::ElementFactory::make("h264parse")
        .name("parser")
        .property_from_str("config-interval", "-1")
        .build()?;
    let sink = gst::ElementFactory::make("appsink")
        .name("sink")
        .property_from_str("max-buffers", "1")
        .property_from_str("drop", "true")
        .property_from_str("sync", "false")
        .build()?;

    let pipeline = gst::Pipeline::with_name("screen-cap");

    pipeline.add_many([
        &source,
        &capsfilter,
        &queue,
        &convert,
        &encoder,
        &parser,
        &sink,
    ])?;

    gst::Element::link_many([
        &source,
        &capsfilter,
        &queue,
        &convert,
        &encoder,
        &parser,
        &sink,
    ])?;

    pipeline.set_state(gst::State::Playing)?;

    // Latency pad probes
    let latency_start = Arc::new(AtomicU64::new(0));
    let latency = Arc::new(AtomicU64::new(0));
    let clock = loop {
        match pipeline.clock() {
            Some(clock) => break clock,
            None => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    };

    let src_pad = source.static_pad("src").unwrap();
    {
        let latency_start = Arc::clone(&latency_start);
        let clock = clock.clone();

        src_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
            if info.buffer().is_some() {
                let now = clock.time().unwrap();
                // *latency_start.lock().unwrap() = now.into();
                latency_start.store(now.into(), Ordering::Release);
                trace!("Captured at: {:?}", now);
            }
            gst::PadProbeReturn::Ok
        });
    }

    let sink_pad = sink.static_pad("sink").unwrap();
    {
        let latency_start = latency_start.clone();
        let latency = latency.clone();
        let clock = clock.clone();
        sink_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, _info| {
            let now: u64 = clock.time().unwrap().into();
            let start_time = latency_start.load(Ordering::Acquire);
            let diff = now - start_time;
            trace!("Latency: {} ms", diff as f64 / 1_000_000.0);
            latency.store(diff, Ordering::Release);
            gst::PadProbeReturn::Ok
        });
    }

    let app_sink = sink.downcast::<gst_app::AppSink>().unwrap();

    // std::thread::sleep(std::time::Duration::from_secs(5));
    // let pad = source.static_pad("src").unwrap();
    // let caps = pad.current_caps().unwrap();
    // println!("{caps:?}");

    // let mut file = std::fs::OpenOptions::new()
    //     .write(true)
    //     .create(true)
    //     .truncate(true)
    //     .open("test.h264")
    //     .unwrap();

    debug!("Screen capture started!");

    std::thread::spawn(move || {
        while let Ok(sample) = app_sink.pull_sample() {
            if let Some(buffer) = sample.buffer() {
                let map = buffer.map_readable().unwrap();

                // let data = map.as_slice();
                // file.write_all(data).unwrap();
                // file.flush().unwrap();

                let lat = latency.load(Ordering::Acquire);
                let mut data = lat.to_be_bytes().to_vec();
                data.extend_from_slice(&map);
                if tx.send(data).is_err() {
                    break; // All receivers dropped
                }
                trace!("Frame sent!");
            }
        }
    });

    let bus = pipeline.bus().unwrap();

    for msg in bus.iter_timed(None) {
        use gst::MessageView;
        match msg.view() {
            MessageView::Error(e) => {
                error!("Gstreamer error: {e}");
                break;
            }
            MessageView::Eos(e) => {
                warn!("EOS detected! {e:?}");
                break;
            }
            _ => (),
        }
    }

    Ok(())
}
