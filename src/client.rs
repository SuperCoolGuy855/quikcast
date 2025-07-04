use log::{debug, error, info, trace, warn};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::watch::{Receiver, Sender};
use tokio::time::Instant;

use color_eyre::eyre::{ContextCompat, bail};
use gstreamer::prelude::*;
use gstreamer::{self as gst, MessageView};
use gstreamer_app::{self as gst_app, AppSrc};
use tokio::sync::watch;

use itertools::Itertools;

use crate::{CLIENT_ARGS, server};

// TODO: Shutdown when windows is closed
pub async fn start_pipeline() -> color_eyre::Result<()> {
    let source = gst::ElementFactory::make("appsrc")
        .name("source")
        .property_from_str("emit-signals", "false")
        .property_from_str("is-live", "true")
        .property_from_str("leaky-type", "2")
        .property_from_str("max-buffers", "1")
        .property_from_str("min-latency", "-1")
        .property_from_str("format", "time")
        .property_from_str("do-timestamp", "true")
        .build()?;
    // let caps = gst::Caps::builder("application/x-rtp")
    //     .field("clock-rate", 90000_i32)
    //     .field("encoding-name", "H264")
    //     .field("media", "video")
    //     .build();
    let caps = gst::Caps::builder("video/x-h264")
        .field("stream-format", "byte-stream")
        .field("alignment", "au")
        .build();
    source.set_property("caps", &caps);
    let payload = gst::ElementFactory::make("rtph264depay")
        .name("payload")
        .build()?;
    let parser = gst::ElementFactory::make("h264parse")
        .name("parser")
        // .property_from_str("disable-passthrough", "true")
        // .property_from_str("config-interval", "-1")
        .build()?;
    let decoder = gst::ElementFactory::make("decodebin")
        .name("decoder")
        .property_from_str("max-size-buffers", "1")
        .build()?;
    let convert = gst::ElementFactory::make_with_name("videoconvert", Some("convert"))?;
    let sink = gst::ElementFactory::make("autovideosink")
        .name("sink")
        .property_from_str("sync", "false")
        .build()?;

    let pipeline = gst::Pipeline::with_name("decoding");
    pipeline.add_many([&source, &payload, &parser, &decoder, &convert, &sink])?;

    gst::Element::link_many([
        &source, // &payload,
        &parser, &decoder,
    ])?;
    gst::Element::link_many([&convert, &sink])?;

    decoder.connect_pad_added(move |src, src_pad| {
        let convert_pad = convert.static_pad("sink").unwrap();

        if convert_pad.is_linked() {
            return;
        }

        if let Some(decodebin) = src.downcast_ref::<gst::Bin>() {
            for element in decodebin.iterate_elements().into_iter().flatten() {
                if let Some(factory) = element.factory() {
                    if factory.klass().contains("Decoder") {
                        debug!("Selected decoder: {}", factory.name());
                    }
                }
            }
        }

        src_pad.link(&convert_pad).unwrap();
    });

    pipeline.set_state(gst::State::Playing)?;

    let (tx, rx) = tokio::sync::watch::channel(Default::default());

    // Latency pad probes
    let latency_start = Arc::new(AtomicU64::new(0));
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
        let clock = clock.clone();
        sink_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, _info| {
            let now: u64 = clock.time().unwrap().into();
            let start_time = latency_start.load(Ordering::Acquire);
            let diff = now - start_time;
            trace!("Latency: {} ms", diff as f64 / 1_000_000.0);
            let decoding_latency = Duration::from_nanos(diff);
            let (encoding_latency, network_latency) = *rx.borrow();
            let total = encoding_latency + decoding_latency + network_latency;
            debug!("Latency: E: {encoding_latency:?} \t N: {network_latency:?} \t D: {decoding_latency:?} \t T: {total:?}");
            gst::PadProbeReturn::Ok
        });
    }

    let mut stream = TcpStream::connect(format!("{}:{}", CLIENT_ARGS.ip, CLIENT_ARGS.port)).await?;
    info!(
        "Connected to server {} with {}",
        stream.peer_addr().unwrap(),
        stream.local_addr().unwrap()
    );

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let udp_port = socket.local_addr()?.port();

    stream.write_u16(udp_port).await?;

    let app_src = source.downcast::<gst_app::AppSrc>().unwrap();

    tokio::spawn(async {
        frame_receive(app_src, socket, tx).await.unwrap();
    });

    let bus = pipeline.bus().unwrap();
    std::thread::spawn(move || {
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
                MessageView::Warning(w) => {
                    warn!("Gstreamer warning: {w:?}");
                }
                _ => (),
            }
        }
    });

    loop {
        match tokio::time::timeout(Duration::from_millis(1000), stream.read_u64()).await {
            Ok(Ok(x)) => {
                trace!("Server heartbeat: {x}");
            }
            Ok(Err(e)) => {
                error!("Server disconnected: {}", e);
                bail!(e);
            }
            Err(_) => {
                error!("Server heartbeat timeout - server likely dead");
                bail!("Heartbeat timeout!");
            }
        }
    }

    pipeline.set_state(gst::State::Null)?;

    Ok(())
}

async fn frame_receive(
    app_src: AppSrc,
    socket: UdpSocket,
    tx: Sender<(Duration, Duration)>,
) -> color_eyre::Result<()> {
    let mut cur_seq_num = 0;
    let mut cur_total_size = 0;
    let mut cur_encoding_latency = 0;
    let mut cur_network_start = 0;
    let mut data = vec![];
    let mut buf = [0; server::CHUNK_SIZE as usize + 50];

    loop {
        if data.len() as u64 >= cur_total_size {
            let mut buffer = gst::Buffer::with_size(data.len())?;
            {
                let mut map = buffer.get_mut().unwrap().map_writable()?;
                map.as_mut_slice().copy_from_slice(&data);
            }
            app_src.push_buffer(buffer)?;
            trace!("Frame pushed!");

            let network_end_time = {
                let time = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap();
                time.as_nanos() as u64
            };

            let encoding_latency = Duration::from_nanos(cur_encoding_latency);
            let network_latency = Duration::from_nanos(network_end_time - cur_network_start);

            tx.send((encoding_latency, network_latency))?;

            data.clear();
            cur_seq_num += 1;
        }

        let size = socket.recv(&mut buf).await?;
        trace!("Data received!");

        match buf[0] {
            255 => {
                if !data.is_empty() {
                    warn!("Incomplete data pushed!");
                    data.clear();
                }
                let (encoding_latency, network_start_time, seq_num, total_size) = buf[1..size]
                    .chunks(8)
                    .map(|x| u64::from_be_bytes(x.try_into().unwrap()))
                    .collect_tuple()
                    .unwrap();
                if cur_seq_num <= seq_num {
                    cur_seq_num = seq_num;
                    cur_total_size = total_size;
                    cur_encoding_latency = encoding_latency;
                    cur_network_start = network_start_time;
                }
            }
            254 => {
                let seq_num = u64::from_be_bytes(buf[1..9].try_into().unwrap());
                // println!("{seq_num} {}", size);
                if seq_num == cur_seq_num {
                    data.extend_from_slice(&buf[9..size]);
                } else {
                    warn!("Out of order data! {seq_num}");
                }
            }
            _ => (),
        }
    }
}
