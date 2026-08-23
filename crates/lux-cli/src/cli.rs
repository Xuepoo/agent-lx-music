use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "alx",
    author,
    version,
    about = "A terminal-native music CLI replacing lx-music-desktop, powered by Agentic intelligence.",
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

    #[arg(
        short = 'd',
        long,
        global = true,
        help = "Enable script engine debug logs"
    )]
    pub debug: bool,

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
    #[command(visible_alias = "sch")]
    Search {
        keyword: String,
        #[arg(short, long, default_value = "all", help = "Filter by source")]
        source: String,
        #[arg(
            short,
            long,
            default_value_t = 1,
            value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..),
            help = "Page number"
        )]
        page: usize,
        #[arg(short, long, default_value_t = 30, help = "Results per page")]
        limit: usize,
        #[arg(long, help = "Output only IDs, one per line")]
        id_only: bool,
    },

    /// Play a song, URL, or local file
    Play {
        #[arg(
            required_unless_present = "from_playlist",
            help = "Song IDs, URLs or local file paths"
        )]
        id_or_url: Vec<String>,
        #[arg(short, long, help = "Override audio quality")]
        quality: Option<String>,
        #[arg(long, help = "Load entire playlist into queue")]
        from_playlist: Option<String>,
        #[arg(long, help = "Shuffle multiple songs when loading")]
        shuffle: bool,
    },

    /// Skip to the next song in the queue
    Next,

    /// Skip to the previous song in the queue
    Prev,

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

    /// Show global player playback state
    State,

    /// Stop mpv and exit daemon
    Quit,

    /// Manage custom JavaScript music sources
    Source {
        #[command(subcommand)]
        action: SourceAction,
    },

    /// Manage background download tasks
    #[command(visible_alias = "dl")]
    Download {
        #[command(subcommand)]
        action: DownloadAction,
    },

    /// Manage custom playlists
    #[command(visible_alias = "pl")]
    Playlist {
        #[command(subcommand)]
        action: PlaylistAction,
    },

    /// Manage local scanned library (supports beets sync)
    Local {
        #[command(subcommand)]
        action: LocalAction,
    },

    /// Manage the active playback queue
    #[command(visible_alias = "q")]
    Queue {
        #[command(subcommand)]
        action: QueueAction,
    },

    /// Manage your favorites playlist
    #[command(visible_alias = "favorites")]
    Fav {
        #[command(subcommand)]
        action: FavAction,
    },

    /// Show play history
    #[command(visible_alias = "hist")]
    History {
        #[arg(short, long, default_value_t = 20, help = "How many entries to show")]
        limit: usize,
    },

    /// Show or save lyrics for a song
    #[command(visible_alias = "lrc", visible_alias = "lyr", visible_alias = "lyrics")]
    Lyric {
        #[arg(
            help = "CLI ID or platform song ID of the song (defaults to currently playing song)"
        )]
        id: Option<String>,
        #[arg(short, long, help = "Display translated lyrics")]
        translated: bool,
        #[arg(short, long, help = "Display romanized lyrics")]
        romanized: bool,
        #[arg(short, long, help = "Save lyrics to .lrc file in download directory")]
        save: bool,
        #[arg(long, help = "Overwrite existing lyrics files when saving")]
        force: bool,
    },

    /// Show or download cover art for a song
    #[command(visible_alias = "cover")]
    Pic {
        #[arg(
            help = "CLI ID or platform song ID of the song (defaults to currently playing song)"
        )]
        id: Option<String>,
        #[arg(short, long, help = "Save cover art to file")]
        save: bool,
        #[arg(short, long, help = "Custom output directory or target path")]
        output: Option<String>,
    },

    /// Generate shell autocompletion scripts
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Browse native leaderboards and hot charts
    #[command(visible_alias = "hot")]
    Board {
        #[arg(short, long, help = "Leaderboard platform source (e.g. wy, kw)")]
        source: Option<String>,
        #[arg(short, long, help = "Specific chart ID to retrieve tracks")]
        id: Option<String>,
        #[arg(long, help = "Play the chart directly")]
        play: bool,
    },

    /// Discover curated playlists and recommendations
    #[command(visible_alias = "explore")]
    Discover {
        #[arg(short, long, help = "Recommended platform source (e.g. wy)")]
        source: Option<String>,
        #[arg(short, long, help = "Tag to filter recommendations (e.g. 华语)")]
        tag: Option<String>,
        #[command(subcommand)]
        action: Option<DiscoverAction>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum QueueAction {
    /// Show current play queue
    Show,
    /// Append song(s) to queue by CLI ID
    Add {
        #[arg(required = true, help = "CLI ID(s) of the song(s)")]
        ids: Vec<String>,
    },
    /// Insert song(s) after current track by CLI ID
    Insert {
        #[arg(required = true, help = "CLI ID(s) of the song(s)")]
        ids: Vec<String>,
    },
    /// Remove song at specified position from queue
    Remove {
        #[arg(required = true, help = "Position in queue (1-indexed)")]
        position: usize,
    },
    /// Clear entire play queue
    Clear,
    /// Move song position in queue
    Move {
        #[arg(required = true, help = "Source position (1-indexed)")]
        from: usize,
        #[arg(required = true, help = "Destination position (1-indexed)")]
        to: usize,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum FavAction {
    /// List all favorites
    List,
    /// Add song to favorites (defaults to currently playing song if ID is omitted)
    Add {
        #[arg(help = "CLI ID of the song")]
        id: Option<String>,
    },
    /// Remove song from favorites by CLI ID
    Remove {
        #[arg(required = true, help = "CLI ID of the song")]
        id: String,
    },
    /// Play all favorites
    Play {
        #[arg(long, help = "Shuffle favorites")]
        shuffle: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DownloadAction {
    /// Queue a song for background download by CLI ID
    Add {
        #[arg(
            required_unless_present = "file",
            help = "CLI ID(s) of the song(s) from search results"
        )]
        ids: Vec<String>,
        #[arg(short, long, help = "Override audio quality")]
        quality: Option<String>,
        #[arg(
            short,
            long,
            help = "Batch download list from playlist file (M3U, CSV, TXT/LIST)"
        )]
        file: Option<String>,
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
    /// Rename an existing playlist
    Rename {
        #[arg(required = true, help = "Current name of the playlist")]
        old_name: String,
        #[arg(required = true, help = "New name for the playlist")]
        new_name: String,
    },
    /// Add song(s) to a playlist by CLI ID
    Add {
        playlist: String,
        #[arg(required = true, help = "CLI ID(s) of the song(s)")]
        ids: Vec<String>,
    },
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
    /// Import playlist from universal file formats (M3U, CSV, TXT/LIST, JSON)
    Import {
        file: String,
        #[arg(short, long, help = "Custom name for imported playlist")]
        name: Option<String>,
        #[arg(long, help = "Download all matched songs immediately")]
        download: bool,
        #[arg(short, long, help = "Request specific high-res quality matching")]
        quality: Option<String>,
    },
    /// Export playlist to universal file formats
    Export {
        name: String,
        #[arg(
            short,
            long,
            default_value = "m3u",
            help = "Output format: m3u, json, csv, txt"
        )]
        format: String,
        #[arg(short, long, help = "Output directory or file path")]
        output: Option<String>,
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
    /// Remove an installed JS source
    Remove { id: String },
    /// Update installed JS sources from their remote URLs
    Update {
        /// Source ID to update
        id: Option<String>,
        /// Check and update all sources
        #[arg(long)]
        all: bool,
    },
    /// Test if a source is functional (Search + URL resolution health checks)
    Test {
        id: String,
        #[arg(long, default_value = "周杰伦")]
        keyword: String,
        #[arg(long)]
        platform: Option<String>,
    },
    /// Show detailed info about an installed source
    Info { id: String },
    /// Check health status and circuit breaker status of registered sources
    Health,
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

#[derive(Subcommand, Debug, Clone)]
pub enum DiscoverAction {
    /// List songs in a recommended playlist
    Show {
        #[arg(required = true, help = "Curated playlist ID")]
        playlist_id: String,
    },
    /// Play a recommended playlist
    Play {
        #[arg(required = true, help = "Curated playlist ID")]
        playlist_id: String,
    },
}
