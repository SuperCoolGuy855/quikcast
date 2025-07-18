use log::{debug, error, info, trace, warn};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{Notify, oneshot, watch};
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::Window;

use crate::{CLIENT_ARGS, server};
use color_eyre::eyre::{ContextCompat, WrapErr, bail};
use gstreamer::prelude::*;
use gstreamer::{self as gst, Pipeline};
use gstreamer_app::AppSrc;
use gstreamer_video::prelude::VideoOverlayExtManual;
use itertools::Itertools;
use tokio::select;

#[derive(Debug, Copy, Clone)]
enum CustomEvent {
    Stop,
}

pub async fn start_client() -> color_eyre::Result<()> {
    let (server_latency_tx, server_latency_rx) = watch::channel(Default::default());
    let stop_flag = AtomicBool::new(false);

    let (socket, stream) = start_network().await?;

    let pipeline = get_pipeline()?;
    let appsrc = get_appsrc_from_pipeline(&pipeline, "source")
        .expect("Appsrc should exist and should be named source");

    let event_loop: EventLoop<CustomEvent> = EventLoop::with_user_event().build()?;
    let proxy_loop_for_async = event_loop.create_proxy();
    let proxy_loop_for_bus = event_loop.create_proxy();

    tokio::spawn(async move {
        select! {
            Err(e) = frame_receive(appsrc, socket, server_latency_tx) => {error!("Can't receive frame: {e}")},
            Err(e) = heartbeat(stream) => {error!("Heartbeat failed! {e}")},
        }
        let _ = proxy_loop_for_async.send_event(CustomEvent::Stop); // This should work, if not then the window has closed already, probably...
    });

    // let pip_clone = pipeline.clone();
    // std::thread::spawn(move || {
    //     bus_monitor(&pip_clone);
    //     debug!("What?");
    //     // proxy_loop_for_bus.send_event(CustomEvent::Stop);
    // });

    start_window(event_loop, &pipeline, server_latency_rx)?;

    pipeline.set_state(gst::State::Null)?;

    Ok(())
}

// FIXME: Pipeline freeze
fn bus_monitor(pipeline: &Pipeline) {
    let bus = pipeline.bus().expect("Pipeline should have a bus");

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
}

fn set_window_id_on_pipeline(pipeline: &Pipeline, window_id: usize) -> color_eyre::Result<()> {
    let bus = pipeline.bus().expect("Pipeline should have a bus");

    bus.set_sync_handler(move |_bus, msg| {
        use gst::MessageView;
        if let MessageView::Element(e) = msg.view() {
            if let Some(structure) = e.structure() {
                if structure.name() == "prepare-window-handle" {
                    if let Some(msg_src) = msg.src()
                        && let Some(overlay) =
                            msg_src.dynamic_cast_ref::<gstreamer_video::VideoOverlay>()
                    {
                        unsafe {
                            overlay.set_window_handle(window_id);
                        }
                    }
                }
            }
        }

        gst::BusSyncReply::Pass
    });

    Ok(())
}

fn get_appsrc_from_pipeline(pipeline: &Pipeline, name: &str) -> color_eyre::Result<AppSrc> {
    let appsrc_element = pipeline
        .by_name(name)
        .wrap_err(format!("Appsrc with name: {name} doesn't exist"))?;
    let appsrc = match appsrc_element.downcast::<AppSrc>() {
        Ok(x) => x,
        Err(e) => bail!("Element with name: {name} is not appsrc. {e:?}"),
    };

    Ok(appsrc)
}

async fn start_network() -> color_eyre::Result<(UdpSocket, TcpStream)> {
    let mut stream = TcpStream::connect(format!("{}:{}", CLIENT_ARGS.ip, CLIENT_ARGS.port)).await?;
    info!(
        "Connected to server {} with {}",
        stream
            .peer_addr()
            .expect("TcpStream is connected and should have a peer address"),
        stream
            .local_addr()
            .expect("TcpStream is connected and should have a local address")
    );

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let udp_port = socket.local_addr()?.port();

    stream.write_u16(udp_port).await?;

    Ok((socket, stream))
}

fn get_pipeline() -> color_eyre::Result<Pipeline> {
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

    let pipeline = Pipeline::with_name("decoding");
    pipeline.add_many([&source, &payload, &parser, &decoder, &convert, &sink])?;

    gst::Element::link_many([
        &source, // &payload,
        &parser, &decoder,
    ])?;
    gst::Element::link_many([&convert, &sink])?;

    decoder.connect_pad_added(move |src, src_pad| {
        let convert_pad = match convert.static_pad("sink") {
            Some(x) => x,
            None => panic!("Unable to get the sink pad from videoconvert element"),
        };

        if convert_pad.is_linked() {
            return;
        }

        if let Some(decodebin) = src.downcast_ref::<gst::Bin>() {
            for element in decodebin.iterate_elements().into_iter().flatten() {
                if let Some(factory) = element.factory() {
                    if factory.klass().contains("Decoder") {
                        info!("Selected decoder: {}", factory.name());
                    }
                }
            }
        }

        match src_pad.link(&convert_pad) {
            Ok(_) => (),
            Err(e) => panic!("Unable to link decodebin src pad with videoconvert sink pad: {e}"),
        }
    });

    Ok(pipeline)
}

fn start_pipeline(
    pipeline: &Pipeline,
    server_latency_rx: watch::Receiver<(Duration, Duration)>,
) -> color_eyre::Result<()> {
    pipeline.set_state(gst::State::Playing)?;
    let bus = pipeline.bus().expect("Pipeline should have a bus");

    for msg in bus.iter_timed(None) {
        use gst::MessageView;
        if let MessageView::NewClock(x) = msg.view() {
            let clock = x.clock().expect("There should be a clock");
            debug!("Pipeline acquired clock: {:?}", clock.name());
            break;
        }
    }

    // Latency pad probes
    let source = pipeline
        .by_name("source")
        .expect("There is a source element in the pipeline");
    let sink = pipeline
        .by_name("source")
        .expect("There is a sink element in the pipeline");
    let latency_start = Arc::new(AtomicU64::new(0));
    let clock = pipeline.clock().expect("checked above");

    if let Some(src_pad) = source.static_pad("src")
        && let Some(sink_pad) = sink.static_pad("sink")
    {
        {
            let latency_start = Arc::clone(&latency_start);
            let clock = clock.clone();

            src_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
                if info.buffer().is_some() {
                    let now = clock
                        .time()
                        .expect("Gstreamer Pipeline Clock should always have a time");
                    // *latency_start.lock().unwrap() = now.into();
                    latency_start.store(now.into(), Ordering::Release);
                    trace!("Captured at: {now:?}");
                }
                gst::PadProbeReturn::Ok
            });
        }

        {
            let latency_start = latency_start.clone();
            let clock = clock.clone();
            sink_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, _info| {
                let now: u64 = clock.time().expect("Gstreamer Pipeline Clock should always have a time").into();
                let start_time = latency_start.load(Ordering::Acquire);
                let diff = now - start_time;
                trace!("Latency: {} ms", diff as f64 / 1_000_000.0);
                let decoding_latency = Duration::from_nanos(diff);
                let (encoding_latency, network_latency) = *server_latency_rx.borrow();
                let total = encoding_latency + decoding_latency + network_latency;
                if CLIENT_ARGS.latency_log {
                    info!("Latency: E: {encoding_latency:?} \t N: {network_latency:?} \t D: {decoding_latency:?} \t T: {total:?}");
                }
                gst::PadProbeReturn::Ok
            });
        }
    }
    Ok(())
}

async fn heartbeat(mut stream: TcpStream) -> color_eyre::Result<()> {
    loop {
        match tokio::time::timeout(Duration::from_millis(1000), stream.read_u64()).await {
            Ok(Ok(x)) => {
                trace!("Server heartbeat: {x}");
            }
            Ok(Err(e)) => {
                bail!("Server disconnected: {e}");
            }
            Err(_) => {
                bail!("Server heartbeat timeout - server likely dead");
            }
        }
    }
}

async fn frame_receive(
    app_src: AppSrc,
    socket: UdpSocket,
    tx: watch::Sender<(Duration, Duration)>,
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
                let mut map = buffer
                    .get_mut()
                    .wrap_err("Unable to get mutable buffer")?
                    .map_writable()?;
                map.as_mut_slice().copy_from_slice(&data);
            }
            app_src.push_buffer(buffer)?;
            trace!("Frame pushed!");

            let network_end_time = {
                let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
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
                    .map(|x| {
                        u64::from_be_bytes(
                            x.try_into()
                                .expect("Number of bytes is checked by chunks()"),
                        )
                    })
                    .collect_tuple()
                    .wrap_err("Incorrect number of u64")?;
                if cur_seq_num <= seq_num {
                    cur_seq_num = seq_num;
                    cur_total_size = total_size;
                    cur_encoding_latency = encoding_latency;
                    cur_network_start = network_start_time;
                }
            }
            254 => {
                let seq_num = u64::from_be_bytes(
                    buf[1..9]
                        .try_into()
                        .expect("Number of bytes is checked by range"),
                );
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

#[derive(Debug)]
struct DisplayApp {
    window: Option<Window>,
    pipeline: Pipeline,
    server_latency_rx: watch::Receiver<(Duration, Duration)>,
}

impl DisplayApp {
    pub fn new(pipeline: &Pipeline, rx: watch::Receiver<(Duration, Duration)>) -> Self {
        Self {
            window: None,
            pipeline: pipeline.clone(),
            server_latency_rx: rx,
        }
    }
}

impl ApplicationHandler<CustomEvent> for DisplayApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let mut attrs = Window::default_attributes();
        attrs.maximized = true;
        self.window = match event_loop.create_window(attrs) {
            Ok(window) => {
                let window_handle = match window.window_handle() {
                    Ok(x) => x,
                    Err(e) => panic!("can't get window handle: {e}"),
                };
                let rwh = window_handle.as_raw();

                // TODO: Wayland display context
                let window_id = {
                    use winit::raw_window_handle::RawWindowHandle;
                    match rwh {
                        RawWindowHandle::Xlib(handle) => handle.window as usize,
                        RawWindowHandle::Wayland(handle) => handle.surface.as_ptr() as usize, // TODO: Add display context too
                        RawWindowHandle::Win32(handle) => handle.hwnd.get() as usize,
                        _ => panic!("unsupported platform"),
                    }
                };

                match set_window_id_on_pipeline(&self.pipeline, window_id) {
                    Ok(_) => (),
                    Err(e) => panic!("Can't set window id to pipeline: {e}"),
                };

                match start_pipeline(&self.pipeline, self.server_latency_rx.clone()) {
                    Ok(_) => (),
                    Err(e) => panic!("Can't start pipeline: {e}"),
                };

                window.set_title("quikcast - client");

                Some(window)
            }
            Err(e) => {
                error!("Unable to create windows: {e}");
                None
            }
        };
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::CloseRequested => {
                debug!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                device_id: _,
                event,
                is_synthetic: _,
            } => {
                if event.state == winit::event::ElementState::Pressed
                    && let winit::keyboard::PhysicalKey::Code(key) = event.physical_key
                    && key == winit::keyboard::KeyCode::KeyF
                    && let Some(window) = &self.window
                {
                    if window.fullscreen().is_none() {
                        window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                    } else {
                        window.set_fullscreen(None);
                    }
                }
            }
            _ => (),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: CustomEvent) {
        match event {
            CustomEvent::Stop => {
                event_loop.exit();
            }
        }
    }
}

fn start_window(
    event_loop: EventLoop<CustomEvent>,
    pipeline: &Pipeline,
    rx: watch::Receiver<(Duration, Duration)>,
) -> color_eyre::Result<()> {
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = DisplayApp::new(pipeline, rx);
    event_loop.run_app(&mut app)?;

    Ok(())
}
