mod cli;
mod cmd;
pub mod library;
pub mod source;

use clap::Parser;
use colored::Colorize;
use std::process;

#[tokio::main]
async fn main() {
    let args = cli::Cli::parse();

    // Setup color override
    let no_color = args.no_color || std::env::var("NO_COLOR").is_ok();
    if no_color {
        colored::control::set_override(false);
    }

    // Determine first run before loading
    let paths = lux_core::config::resolve_paths();
    let is_first_run = !paths.config_file.exists();

    // Dispatch subcommand execution
    match cmd::dispatch(args.command, args.json) {
        Ok(_) => {
            if is_first_run && !args.quiet && !args.json {
                println!("\n{} {}!", "✓".green().bold(), "rust-lx initialized".bold());
                println!("  Config: {}", paths.config_file.display());
                println!("  Data:   {}", paths.data_dir.display());
                println!("\nGet started:");
                println!("  rlx config                  Show current config");
                println!("  rlx search <keyword>        Search for music");
                println!("  rlx play <id>               Play a song\n");
            }
            process::exit(0);
        }
        Err(e) => {
            if args.json {
                eprintln!("{}", serde_json::json!({ "error": e.to_string() }));
            } else {
                eprintln!("{} {}", "error:".red().bold(), e);
            }
            process::exit(1);
        }
    }
}
