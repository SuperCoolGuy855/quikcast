use std::sync::LazyLock;

use clap::{Args, Parser, Subcommand};
use gstreamer::prelude::PluginFeatureExtManual;

mod client;
mod screen_cap;
mod server;

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct CliArgs {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    #[command(about = "Start in server mode")]
    Server(ServerArgs),
    #[command(about = "Start in client mode")]
    Client(ClientArgs),
}

#[derive(Args, Debug, Clone)]
struct ServerArgs {
    #[arg(short, long, default_value = "18900", help = "TCP port to bind")]
    port: u16,

    #[arg(short, long, default_value = "0.0.0.0", help = "Optional IP address to bind")]
    ip: String,
}

#[derive(Args, Debug, Clone)]
struct ClientArgs {
    #[arg(short, long, default_value = "18900", help = "Server TCP port to connect")]
    port: u16,

    #[arg(short, long, help = "Server IP address to connect")]
    ip: String,
}

static SERVER_ARGS: LazyLock<ServerArgs> = LazyLock::new(|| match CliArgs::parse().command {
    Commands::Server(x) => x,
    _ => panic!("Client command not used"),
});

static CLIENT_ARGS: LazyLock<ClientArgs> = LazyLock::new(|| match CliArgs::parse().command {
    Commands::Client(x) => x,
    _ => panic!("Server command not used"),
});

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

    let args = CliArgs::parse();
    match args.command {
        Commands::Server { .. } => server::start_server().await?,
        Commands::Client { .. } => client::start_pipeline().await?,
    }

    Ok(())
}
