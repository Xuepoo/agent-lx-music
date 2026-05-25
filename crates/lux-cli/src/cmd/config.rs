use crate::cli::ConfigAction;
use anyhow::{Result, anyhow};

pub fn run(action: Option<ConfigAction>, json: bool) -> Result<()> {
    let paths = lux_core::config::resolve_paths();
    let action = action.unwrap_or(ConfigAction::Show);

    match action {
        ConfigAction::Path => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "path": paths.config_file.to_string_lossy() })
                );
            } else {
                println!("{}", paths.config_file.display());
            }
        }
        ConfigAction::Show => {
            let config = lux_core::config::Config::load()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&config)?);
            } else {
                let toml_str = toml::to_string_pretty(&config)?;
                println!("{}", toml_str);
            }
        }
        ConfigAction::Get { key } => {
            let config = lux_core::config::Config::load()?;
            let val = get_config_val(&config, &key)?;
            if json {
                println!("{}", serde_json::json!({ key.as_str(): val }));
            } else {
                println!("{}", val);
            }
        }
        ConfigAction::Set { key, value } => {
            let mut config = lux_core::config::Config::load()?;
            set_config_val(&mut config, &key, &value)?;
            config.save()?;
            if !json {
                println!("✓ Config '{}' successfully updated to '{}'", key, value);
            }
        }
    }
    Ok(())
}

fn get_config_val(config: &lux_core::config::Config, key: &str) -> Result<String> {
    match key {
        "source.default_source" => Ok(config.source.default_source.clone()),
        "source.default_quality" => Ok(config.source.default_quality.as_str().to_string()),
        "player.default_volume" => Ok(config.player.default_volume.to_string()),
        "player.repeat" => Ok(config.player.repeat.clone()),
        "player.shuffle" => Ok(config.player.shuffle.to_string()),
        "player.enable_mpris" => Ok(config.player.enable_mpris.to_string()),
        "player.auto_resume" => Ok(config.player.auto_resume.to_string()),
        "download.output_dir" => Ok(config.download.output_dir.clone()),
        "download.filename_template" => Ok(config.download.filename_template.clone()),
        "download.beet_import" => Ok(config.download.beet_import.to_string()),
        "download.use_beets_library" => Ok(config.download.use_beets_library.to_string()),
        "history.max_age_days" => Ok(config.history.max_age_days.to_string()),
        "display.color" => Ok(config.display.color.clone()),
        "display.table_style" => Ok(config.display.table_style.clone()),
        _ => Err(anyhow!("Unsupported or read-only config key: {}", key)),
    }
}

fn set_config_val(config: &mut lux_core::config::Config, key: &str, value: &str) -> Result<()> {
    match key {
        "source.default_source" => {
            config.source.default_source = value.to_string();
        }
        "source.default_quality" => {
            use std::str::FromStr;
            config.source.default_quality =
                lux_core::types::Quality::from_str(value).map_err(|e| anyhow!("{}", e))?;
        }
        "player.default_volume" => {
            config.player.default_volume = value.parse::<u8>()?;
        }
        "player.repeat" => {
            config.player.repeat = value.to_string();
        }
        "player.shuffle" => {
            config.player.shuffle = value.parse::<bool>()?;
        }
        "player.enable_mpris" => {
            config.player.enable_mpris = value.parse::<bool>()?;
        }
        "player.auto_resume" => {
            config.player.auto_resume = value.parse::<bool>()?;
        }
        "download.output_dir" => {
            config.download.output_dir = value.to_string();
        }
        "download.filename_template" => {
            config.download.filename_template = value.to_string();
        }
        "download.beet_import" => {
            config.download.beet_import = value.parse::<bool>()?;
        }
        "download.use_beets_library" => {
            config.download.use_beets_library = value.parse::<bool>()?;
        }
        "history.max_age_days" => {
            config.history.max_age_days = value.parse::<u64>()?;
        }
        "display.color" => {
            config.display.color = value.to_string();
        }
        "display.table_style" => {
            config.display.table_style = value.to_string();
        }
        _ => return Err(anyhow!("Unsupported or read-only config key: {}", key)),
    }
    Ok(())
}
