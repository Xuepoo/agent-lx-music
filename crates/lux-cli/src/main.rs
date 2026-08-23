use agent_lx_music::{cli, cmd};
use clap::Parser;
use colored::Colorize;
use std::process;

#[tokio::main]
async fn main() {
    let args = cli::Cli::parse();

    // Dynamically inject custom config path into environment to globally override paths
    if let Some(ref custom_config) = args.config {
        unsafe {
            std::env::set_var("ALX_CONFIG", custom_config);
        }
    }

    if args.verbose {
        unsafe {
            std::env::set_var("ALX_VERBOSE", "1");
        }
    }

    // Setup color override
    let no_color = args.no_color || std::env::var("NO_COLOR").is_ok();
    if no_color {
        colored::control::set_override(false);
    }

    // Determine first run before loading
    let paths = lux_core::config::resolve_paths();
    let is_first_run = !paths.config_file.exists();

    // The MCP server owns stdout for protocol frames; nothing else may
    // write there, including the first-run banner.
    let is_mcp = matches!(args.command, cli::Commands::Mcp);

    // Dispatch subcommand execution
    match cmd::dispatch(args.command, args.json, args.quiet).await {
        Ok(_) => {
            if is_first_run && !args.quiet && !args.json && !is_mcp {
                println!(
                    "\n{} {}!",
                    "✓".green().bold(),
                    "agent-lx-music initialized".bold()
                );
                println!("  Config: {}", paths.config_file.display());
                println!("  Data:   {}", paths.data_dir.display());
                println!("\nGet started:");
                println!("  alx config                  Show current config");
                println!("  alx search <keyword>        Search for music");
                println!("  alx play <id>               Play a song\n");
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
