use std::net::{IpAddr, Ipv4Addr};
use std::thread;

use clap::{Parser, Subcommand};
use color_eyre::eyre::bail;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::watch;

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

    let args = Cli::parse();
    if args.server {
        if let Some(ip) = args.ip {
            server(ip, args.port).await?;
        } else {
            bail!("Server mode needs an IP to send data");
        }
    } else if args.client {
        client(args.port).await?;
    }

    Ok(())
}

async fn server(ip: String, port: u16) -> color_eyre::Result<()> {
    let (tx, mut rx) = watch::channel(vec![]);
    std::thread::spawn(move || screen_cap::start_pipeline(tx));
    let mut stream = TcpStream::connect(format!("{ip}:{port}")).await?;
    loop {
        if rx.changed().await.is_err() {
            break;
        }
        let data = rx.borrow_and_update();
        let size = data.len() as u64;
        stream.write_u64(size).await?;
        stream.write_all(&data).await?;
    }
    Ok(())
}

async fn client(port: u16) -> color_eyre::Result<()> {
    client::start_pipeline(port).await
}
