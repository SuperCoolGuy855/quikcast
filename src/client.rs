use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::sync::watch::Receiver;
use tokio::net::{TcpListener, UdpSocket};

use color_eyre::eyre::{ContextCompat, bail};
use gstreamer::prelude::*;
use gstreamer::{self as gst, MessageView};
use gstreamer_app as gst_app;
use tokio::sync::watch;

pub async fn start_pipeline(port: u16) -> color_eyre::Result<()> {
    let source = gst::ElementFactory::make("appsrc")
        .name("source")
        .property_from_str("emit-signals", "true")
        .property_from_str("is-live", "true")
        .property_from_str("leaky-type", "2")
        .property_from_str("max-buffers", "1")
        .property_from_str("min-latency", "-1")
        .property_from_str("format", "time")
        .property_from_str("do-timestamp", "true")
        .build()?;
    let caps = gst::Caps::builder("video/x-h264")
        .build();
    source.set_property("caps", &caps);
    let parser = gst::ElementFactory::make_with_name("h264parse", Some("parser"))?;
    let decoder = gst::ElementFactory::make("decodebin")
        .name("decoder")
        .property_from_str("max-size-buffers", "1")
        .build()?;
    let convert = gst::ElementFactory::make_with_name("videoconvert", Some("convert"))?;
    let sink = gst::ElementFactory::make_with_name("autovideosink", Some("sink"))?;

    let pipeline = gst::Pipeline::with_name("decoding");
    pipeline.add_many([&source, &parser, &decoder, &convert, &sink])?;

    gst::Element::link_many([&source, &parser, &decoder])?;
    gst::Element::link_many([&convert, &sink])?;

    decoder.connect_pad_added(move |src, src_pad| {
        let convert_pad = convert.static_pad("sink").unwrap();
        
        if convert_pad.is_linked() {
            return;
        }

        src_pad.link(&convert_pad).unwrap();
    });

    pipeline.set_state(gst::State::Playing)?;

    let app_src = source.downcast::<gst_app::AppSrc>().unwrap();
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    let (mut stream, _) = listener.accept().await?;
    loop {
        let size = stream.read_u64().await? as usize;
        let mut data = vec![0; size];
        stream.read_exact(&mut data).await?;
        
        let mut buffer = gst::Buffer::with_size(size)?;
        {
            let mut map = buffer.get_mut().unwrap().map_writable()?;
            map.as_mut_slice().copy_from_slice(&data);
        }
        
        app_src.push_buffer(buffer)?;
    }
    Ok(())
}
