use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::thread;
use std::time::SystemTime;

use clap::{Parser, Subcommand};
use color_eyre::eyre::bail;
use gstreamer::prelude::PluginFeatureExtManual;
use itertools::Itertools;
use log::{debug, info, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::watch::{self, Receiver};

mod client;
mod screen_cap;
mod server;

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    server: bool,

    #[arg(short, long)]
    client: bool,

    #[arg(short, long)]
    port: u16,

    #[arg(short, long)]
    ip: Option<String>,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    gstreamer::init()?;

    // Lower priority of openh264dec because I can't deal with openh264 anymore
    // Get the default registry
    let registry = gstreamer::Registry::get();
    if let Some(plugin_feature) = registry.lookup_feature("openh264dec") {
        plugin_feature.set_rank(gstreamer::Rank::from(1));
    }

    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        // .filter_module("quikcast::server", log::LevelFilter::Trace)
        .filter_module("quikcast::client", log::LevelFilter::Debug)
        .init();

    let args = Cli::parse();
    if args.server {
        server::start_server(args.port).await?;
    } else if args.client {
        if let Some(ip) = args.ip {
            client::start_pipeline(ip, args.port).await?;
        } else {
            bail!("Expected IP address!");
        }
    }

    Ok(())
}
