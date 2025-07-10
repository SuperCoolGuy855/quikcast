# quikcast

A simple, fast, reasonably high quality, low-latency, but high bandwidth screen casting/streaming solution using H264 codecs, Gstreamer, and Rust.

## Features
- **Low Latency**: Designed for real-time applications.
- **High Quality**: Uses H264 codecs for efficient compression, higher quality, and lower latency.
- **High Bandwidth**: Optimized for high bandwidth connections, suitable for local networks.

## Requirements
- **Rust**: Ensure you have Rust installed. You can install it from [rustup.rs](https://rustup.rs/).
- **GStreamer**: You need GStreamer installed with the necessary plugins for H264 encoding and decoding, and rendering/display. You can install it from [GStreamer](https://gstreamer.freedesktop.org/download/). Also read the Gstreamer Rust crate for more details: [gstreamer](https://crates.io/crates/gstreamer#installation).

## Current Limitations / TODOs
- **OS Support**: Currently, the server is only for **Windows**, but the client **should work** on any OS that supports GStreamer. Cross-platform support is planned.
- **No Audio Support**: Currently, only video streaming is supported.
- **No GUI**: The application is command-line based.
- **No Authentication**: There is no authentication mechanism, so anyone on the network can connect.
- **No Encryption**: The data is sent unencrypted, which may not be suitable for sensitive information.
- **No Error Handling**: The code lacks robust error handling, which may lead to crashes or undefined behavior in case of network issues or other errors.
- **No Adaptive Bitrate**: The bitrate is fixed, which may not be optimal for varying network conditions.

## Installation
Install directly from the source code:

```shell
cargo install --git https://github.com/SuperCoolGuy855/quikcast.git
```

## Usage
Run the server on the machine you want to stream from:

```shell
quikcast server
```

Run the client on the machine you want to stream to:

```shell
quikcast client <server_ip/hostname>
```

Please refer to the help command for options and usage details:

```shell
quikcast --help / quikcast server --help / quikcast client --help
```