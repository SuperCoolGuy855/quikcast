use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::thread;
use std::time::SystemTime;

use clap::{Parser, Subcommand};
use color_eyre::eyre::bail;
use itertools::Itertools;
use log::{debug, info, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::watch::{self, Receiver};

mod client;
mod screen_cap;

#[derive(Parser)]
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
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let args = Cli::parse();
    if args.server {
        server(args.port).await?;
    } else if args.client {
        if let Some(ip) = args.ip {
            client::start_pipeline(ip, args.port).await?;
        } else {
            panic!();
        }
    }

    Ok(())
}

async fn server(port: u16) -> color_eyre::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("TCP server started on {}", listener.local_addr().unwrap());

    let (tx, rx) = watch::channel(vec![]);
    std::thread::spawn(move || screen_cap::start_pipeline(tx));
    debug!("Screen capture starting!");

    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        debug!("New connection: {addr}");
        let rx_clone = rx.clone();
        tokio::spawn(connection(stream, addr, rx_clone));
    }
}

const CHUNK_SIZE: u64 = 1200;

async fn connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    mut rx: Receiver<Vec<u8>>,
) -> color_eyre::Result<()> {
    let udp_port = stream.read_u16().await?;

    let mut udp_addr = addr;
    udp_addr.set_port(udp_port);
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(udp_addr).await?;

    info!(
        "Connected UDP to {} with {}",
        socket.local_addr().unwrap(),
        udp_addr
    );

    let mut seq_num: u64 = 1;
    loop {
        rx.changed().await?;
        trace!("New Frame!");
        let mut data = rx.borrow_and_update().to_vec();
        let encoding_latency_bytes = data.drain(..8).collect_vec();
        let size = data.len() as u64;

        let network_start_time = {
            let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
            time.as_nanos() as u64
        };

        let mut header = Vec::with_capacity(4 * 8 + 1);
        let seq_num_byte = seq_num.to_be_bytes();
        header.push(255);
        // header.extend_from_slice(&CHUNK_SIZE.to_be_bytes());
        header.extend(encoding_latency_bytes);
        header.extend(network_start_time.to_be_bytes());
        header.extend_from_slice(&seq_num_byte);
        header.extend(size.to_be_bytes());
        socket.send(&header).await?;

        for chunk in data.chunks(CHUNK_SIZE as usize) {
            let mut data = Vec::with_capacity(chunk.len() + 1 + 8);
            data.push(254);
            data.extend_from_slice(&seq_num_byte);
            data.extend_from_slice(chunk);

            socket.send(&data).await?;
        }

        seq_num += 1;
    }
    Ok(())
}
