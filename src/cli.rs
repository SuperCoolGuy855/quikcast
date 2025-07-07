use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
pub struct CliArgs {
    #[arg(short, long, help = "Enable debug logging")]
    pub logs: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    #[command(about = "Start in server mode")]
    Server(ServerArgs),
    #[command(about = "Start in client mode")]
    Client(ClientArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ServerArgs {
    #[arg(
        short,
        long,
        default_value = "0.0.0.0",
        help = "Optional IP address to bind"
    )]
    pub ip: String,

    #[arg(short, long, default_value = "18900", help = "TCP port to bind")]
    pub port: u16,

    #[arg(
        long = "index",
        default_value = "-1",
        help = "Monitor index to cast. Default to primary monitor"
    )]
    pub monitor_index: i32,

    #[arg(
        value_enum,
        long = "api",
        default_value = "dxgi",
        help = "Screen Capture API"
    )]
    pub capture_api: CaptureApi,

    #[arg(long, default_value = "30", help = "Capture frame rate")]
    pub framerate: u32,

    #[arg(long, default_value = "20000", help = "Stream bitrate")]
    pub bitrate: u32,

    #[arg(
        long,
        default_value = "10",
        help = "Number of frames between intra frames (-1 = infinite)"
    )]
    pub gop_size: i32,

    #[arg(value_enum, long, default_value = "cbr", help = "Rate Control Mode")]
    pub rc_mode: RcMode,

    #[arg(value_enum, long, default_value = "p1", help = "Encoding Preset")]
    pub preset: Preset,

    #[arg(
        long,
        default_value = "8",
        help = "Adaptive Quantization Strength when spatial-aq is enabled from 1 (low) to 15 (aggressive), (0 = autoselect)"
    )]
    pub aq_strength: u32, // TODO: Value check

    #[arg(long, help = "Disable Spatial Adaptive Quantization")]
    pub no_spatial_aq: bool,

    #[arg(long, help = "Disable Temporal Adaptive Quantization")]
    pub no_temporal_aq: bool,

    #[arg(long, help = "Disable Zero latency operation")]
    pub no_zerolatency: bool,

    #[arg(long, help = "Don't insert sequence headers (SPS/PPS) per IDR")]
    pub no_repeat_header: bool,

    #[arg(long, help = "Don't capture mouse cursor")]
    pub no_cursor: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ClientArgs {
    #[arg(help = "Server IP address to connect")]
    pub ip: String,

    #[arg(
        short,
        long,
        default_value = "18900",
        help = "Server TCP port to connect"
    )]
    pub port: u16,

    #[arg(long, help = "*Try* to enable fullscreen")]
    pub fullscreen: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CaptureApi {
    Dxgi,
    Wgc,
}

impl From<CaptureApi> for &str {
    fn from(value: CaptureApi) -> Self {
        match value {
            CaptureApi::Dxgi => "0",
            CaptureApi::Wgc => "1",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RcMode {
    Default,
    ConstQp,
    Cbr,
    Vbr,
}

impl From<RcMode> for &str {
    fn from(value: RcMode) -> Self {
        match value {
            RcMode::Default => "0",
            RcMode::ConstQp => "1",
            RcMode::Cbr => "2",
            RcMode::Vbr => "3",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Preset {
    P1,
    P2,
    P3,
    P4,
    P5,
    P6,
    P7,
}

impl From<Preset> for &str {
    fn from(value: Preset) -> Self {
        match value {
            Preset::P1 => "8",
            Preset::P2 => "9",
            Preset::P3 => "10",
            Preset::P4 => "11",
            Preset::P5 => "12",
            Preset::P6 => "13",
            Preset::P7 => "14",
        }
    }
}
