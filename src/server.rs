use std::{net::SocketAddr, time::SystemTime};

use itertools::Itertools;
use log::{debug, info, trace};
use tokio::{
    io::AsyncReadExt, net::{TcpListener, TcpStream, UdpSocket}, sync::watch::{self, Receiver, Sender}
};

use crate::screen_cap;

pub async fn start_server(port: u16) -> color_eyre::Result<()> {
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
            let time = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap();
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
        trace!("Header sent!");

        for chunk in data.chunks(CHUNK_SIZE as usize) {
            let mut data = Vec::with_capacity(chunk.len() + 1 + 8);
            data.push(254);
            data.extend_from_slice(&seq_num_byte);
            data.extend_from_slice(chunk);

            socket.send(&data).await?;
            trace!("Data sent!");
        }

        seq_num += 1;
    }
    Ok(())
}
