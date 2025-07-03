use std::sync::LazyLock;

use clap::{Args, Parser, Subcommand};
use gstreamer::prelude::PluginFeatureExtManual;

use crate::cli::*;

mod cli;
mod client;
mod screen_cap;
mod server;

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

    if cfg!(debug_assertions) {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            // .filter_module("quikcast::server", log::LevelFilter::Trace)
            .filter_module("quikcast::client", log::LevelFilter::Debug)
            .init();
    } else {
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .init();
    }

    let args = CliArgs::parse();
    match args.command {
        Commands::Server { .. } => server::start_server().await?,
        Commands::Client { .. } => client::start_pipeline().await?,
    }

    Ok(())
}
