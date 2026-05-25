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

    /// Pause playback
    Pause,

    /// Resume playback
    Resume,

    /// Stop playback
    Stop,

    /// Set or show volume
    Volume {
        #[arg(help = "Volume value (e.g. 80, +10, -10)")]
        value: Option<String>,
    },

    /// Seek to position
    Seek {
        #[arg(required = true, help = "Seek position (e.g. 2:30, +30, -10, 50%)")]
        value: String,
    },

    /// Set or show repeat mode
    Repeat {
        #[arg(help = "Repeat mode (off, one, all)")]
        mode: Option<String>,
    },

    /// Toggle or set shuffle mode
    Shuffle {
        #[arg(help = "Shuffle mode (on, off)")]
        mode: Option<String>,
    },

    /// Stop mpv and exit daemon
    Quit,

    /// Manage custom JavaScript music sources
    Source {
        #[command(subcommand)]
        action: SourceAction,
    },

    /// Manage background download tasks
    Download {
        #[command(subcommand)]
        action: DownloadAction,
    },

    /// Manage custom playlists
    Playlist {
        #[command(subcommand)]
        action: PlaylistAction,
    },

    /// Manage local scanned library (supports beets sync)
    Local {
        #[command(subcommand)]
        action: LocalAction,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DownloadAction {
    /// Queue a song for background download by CLI ID
    Add {
        #[arg(required = true, help = "CLI ID(s) of the song(s) from search results")]
        ids: Vec<String>,
        #[arg(short, long, help = "Override audio quality")]
        quality: Option<String>,
    },
    /// Start the detached background download daemon (used internally)
    Daemon,
    /// View real-time status of active downloads
    Status,
    /// List history of all completed and failed downloads
    List,
    /// Retry a failed download task by task ID
    Retry {
        #[arg(required = true, help = "Task ID(s) to retry")]
        ids: Vec<i64>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum PlaylistAction {
    /// List all playlists
    List,
    /// Create a new playlist
    Create {
        name: String,
        description: Option<String>,
    },
    /// Delete an existing playlist
    Delete { name: String },
    /// Add a song to a playlist by CLI ID
    Add { playlist: String, id: String },
    /// Remove a song from a playlist by CLI ID
    Remove { playlist: String, id: String },
    /// Show all songs in a playlist
    Show { name: String },
    /// Play all tracks in a playlist
    Play {
        name: String,
        #[arg(long, help = "Shuffle tracks")]
        shuffle: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum LocalAction {
    /// Scan local music directory or beets library to index files
    Scan,
    /// List all indexed local music
    List,
    /// Play a local song by index or filename
    Play {
        #[arg(required = true, help = "Index or filename of the local song")]
        query: String,
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
