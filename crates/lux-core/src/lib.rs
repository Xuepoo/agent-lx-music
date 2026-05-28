pub mod config;
pub mod error;
pub mod traits;
pub mod types;

#[macro_export]
macro_rules! log_verbose {
    ($($arg:tt)*) => {
        if std::env::var("ALX_VERBOSE").is_ok() {
            eprintln!("\x1b[1;35m[DEBUG]\x1b[0m {}", format!($($arg)*));
        }
    };
}
