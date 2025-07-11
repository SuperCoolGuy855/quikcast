use std::{
    net::SocketAddr,
    time::{Duration, SystemTime},
};

use itertools::Itertools;
use log::{debug, error, info, trace, warn};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::watch::{self, Receiver},
};

use crate::{SERVER_ARGS, screen_cap};

pub async fn start_server() -> color_eyre::Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", SERVER_ARGS.ip, SERVER_ARGS.port)).await?;
    info!(
        "TCP server started on {}",
        listener
            .local_addr()
            .expect("Expected to have IP after binding")
    );

    let (tx, rx) = watch::channel(vec![]);

    std::thread::spawn(move || screen_cap::start_pipeline(tx)); // TODO: Crash when this crash
    debug!("Screen capture starting!");

    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                error!("Unable to accept incoming connection: {e}");
                continue;
            },
        };
        debug!("New connection: {addr}");
        let rx_clone = rx.clone();
        tokio::spawn(connection(stream, addr, rx_clone));
    }
}

pub const CHUNK_SIZE: u64 = 1200;
async fn connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    rx: Receiver<Vec<u8>>,
) {
    let udp_port = match stream.read_u16().await {
        Ok(x) => x,
        Err(e) => {
            warn!("Can't get udp port from client: {e}");
            return;
        },
    };

    let mut udp_addr = addr;
    udp_addr.set_port(udp_port);

    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(x) => x,
        Err(e) => {
            warn!("Can't bind UdpSocket: {e}");
            return;
        },
    };

    if let Err(e) = socket.connect(udp_addr).await {
        warn!("Can't connect UDP to {udp_addr}: {e}");
        return;
    }

    info!(
        "Connected UDP to {} with {}",
        socket.local_addr().expect("Expected to have IP after binding"),
        udp_addr
    );

    tokio::select! {
        color_eyre::Result::Err(e) = send_frame(socket, rx) => {warn!("Unable to send data/frame to client {addr}: {e}")},
        color_eyre::Result::Err(e) = heartbeat(stream) => {warn!("Client {addr} disconnected: {e}");}
    }
}

async fn heartbeat(mut stream: TcpStream) -> color_eyre::Result<()> {
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    let mut heartbeat_index = 0;
    loop {
        interval.tick().await;

        stream.write_u64(heartbeat_index).await?;

        heartbeat_index += 1;
    }
}

async fn send_frame(socket: UdpSocket, mut rx: Receiver<Vec<u8>>) -> color_eyre::Result<()> {
    let mut seq_num: u64 = 1; // This start as one for tracking purpose
    // let mut prev_index_num = None;
    loop {
        // let start_time = Instant::now();
        rx.changed().await?;
        trace!("New Frame!");
        let mut data = rx.borrow_and_update().to_vec();
        let encoding_latency_bytes = data.drain(..8).collect_vec();

        // let index_num_bytes = data.drain(..8).collect_array().unwrap();
        // let index_num = u64::from_be_bytes(index_num_bytes);

        // if let Some(prev_index_num) = prev_index_num {
        //     if prev_index_num + 1 != index_num {
        //         warn!("Mismatch: {prev_index_num} {index_num}");
        //     }
        // }
        // prev_index_num = Some(index_num);

        let size = data.len() as u64;
        // println!("Send: {num} {size}");

        let network_start_time = {
            let time = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default(); // TODO: Check if this is good
            time.as_nanos() as u64
        };

        // if rand::random_bool(0.5) {
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
            // println!("{seq_num} {}", data.len());
            trace!("Data sent!");
        }
        // }

        // debug!("{:?}", Instant::now() - start_time);

        seq_num += 1;
    }
}
