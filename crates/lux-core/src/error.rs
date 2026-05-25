use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum SourceError {
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("Platform rate limit reached")]
    RateLimited,

    #[error("Unsupported music quality")]
    UnsupportedQuality,

    #[error("JS execution error: {0}")]
    ScriptError(String),

    #[error("Platform parser error: {0}")]
    PlatformError(String),

    #[error("Song not found")]
    NotFound,

    #[error("No downloadable URL resolved")]
    NoUrl,
}

#[derive(Error, Debug)]
pub enum LuxError {
    #[error("Source error: {0}")]
    Source(#[from] SourceError),

    #[error("Player error: {0}")]
    Player(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Download error: {0}")]
    Download(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Unknown error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LuxError>;
