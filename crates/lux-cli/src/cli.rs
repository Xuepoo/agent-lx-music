use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "rlx",
    author,
    version,
    about = "A terminal-native music CLI replacing lx-music-desktop",
    long_about = None
)]
pub struct Cli {
    #[arg(short, long, global = true, help = "Path to custom config file")]
    pub config: Option<String>,

    #[arg(short, long, global = true, help = "Suppress non-data output")]
    pub quiet: bool,

    #[arg(long, global = true, help = "Output as JSON")]
    pub json: bool,

    #[arg(long, global = true, help = "Disable colored output")]
    pub no_color: bool,

    #[arg(short, long, global = true, help = "Verbose logging")]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Show or modify application configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Search for music across sources
    Search {
        keyword: String,
        #[arg(short, long, default_value = "all", help = "Filter by source")]
        source: String,
        #[arg(short, long, default_value_t = 1, help = "Page number")]
        page: usize,
        #[arg(short, long, default_value_t = 30, help = "Results per page")]
        limit: usize,
        #[arg(long, help = "Output only IDs, one per line")]
        id_only: bool,
    },

    /// Play a song, URL, or local file
    Play {
        #[arg(required = true, help = "Song IDs, URLs or local file paths")]
        id_or_url: Vec<String>,
        #[arg(short, long, help = "Override audio quality")]
        quality: Option<String>,
        #[arg(long, help = "Load entire playlist into queue")]
        from_playlist: Option<String>,
        #[arg(long, help = "Shuffle multiple songs when loading")]
        shuffle: bool,
    },

    /// Show current playback status
    Now,

    /// Manage custom JavaScript music sources
    Source {
        #[command(subcommand)]
        action: SourceAction,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum SourceAction {
    /// Add an installed JS source script
    Add { path_or_url: String },
    /// List all installed JS sources
    List,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigAction {
    /// Show current config (default action)
    Show,
    /// Get a config value
    Get { key: String },
    /// Set a config value
    Set { key: String, value: String },
    /// Show config file path
    Path,
}
