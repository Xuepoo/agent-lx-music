use crate::cli::ConfigAction;
use anyhow::{Result, anyhow};
use std::path::Path;

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
        ConfigAction::Edit => {
            // Materialize the default config first so the editor never opens
            // a missing file and saves it into an unexpected location.
            if !paths.config_file.exists() {
                lux_core::config::Config::load()?;
            }
            let editor = editor_program();
            run_edit_with(&paths.config_file.clone(), &editor, spawn_editor)?;
            if !json {
                println!(
                    "✓ Config '{}' saved and validated",
                    paths.config_file.display()
                );
            }
        }
    }
    Ok(())
}

/// Resolve documented short aliases (docs/cli.md) to their canonical dotted
/// keys. Only aliases present in the published docs are supported.
fn canonical_config_key(key: &str) -> &str {
    match key {
        "default_quality" => "source.default_quality",
        other => other,
    }
}

/// Editor selection for `alx config edit`: $VISUAL, then $EDITOR, then vi.
fn editor_program() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string())
}

/// Spawn the editor attached to the terminal and wait for it to finish.
fn spawn_editor(program: &str, path: &Path) -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new(program).arg(path).status()
}

/// Run the editor via the injectable [`spawner`] seam, then re-read and
/// validate the config; any failure surfaces as a non-zero CLI exit.
pub(crate) fn run_edit_with<F>(config_path: &Path, editor: &str, spawner: F) -> Result<()>
where
    F: FnOnce(&str, &Path) -> std::io::Result<std::process::ExitStatus>,
{
    let status = spawner(editor, config_path)
        .map_err(|e| anyhow!("Failed to launch editor '{}': {}", editor, e))?;
    if !status.success() {
        return Err(anyhow!(
            "Editor '{}' exited unsuccessfully; skipping config validation",
            editor
        ));
    }
    validate_config_file(config_path)
}

fn validate_config_file(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read edited config file: {}", e))?;
    let _: lux_core::config::Config = toml::from_str(&content).map_err(|e| {
        anyhow!(
            "Edited config is invalid TOML or contains unknown keys: {}",
            e
        )
    })?;
    Ok(())
}

fn get_config_val(config: &lux_core::config::Config, key: &str) -> Result<String> {
    match canonical_config_key(key) {
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
    match canonical_config_key(key) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static EDIT_TEST_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_canonical_config_key_aliases() {
        // Documented short alias maps to its canonical dotted key.
        assert_eq!(
            canonical_config_key("default_quality"),
            "source.default_quality"
        );
        // Canonical keys and unknown keys pass through unchanged.
        assert_eq!(
            canonical_config_key("source.default_quality"),
            "source.default_quality"
        );
        assert_eq!(canonical_config_key("player.repeat"), "player.repeat");
        assert_eq!(canonical_config_key("not_a_key"), "not_a_key");
        // Only documented aliases are supported; no dotted-lookalike traps.
        assert_eq!(canonical_config_key("default_source"), "default_source");
    }

    #[test]
    fn test_alias_roundtrip_through_get_and_set() {
        let mut config = lux_core::config::Config::default();

        let quality = get_config_val(&config, "default_quality").unwrap();
        assert_eq!(quality, "320k");

        set_config_val(&mut config, "default_quality", "flac").unwrap();
        assert_eq!(
            get_config_val(&config, "source.default_quality").unwrap(),
            "flac"
        );

        // Unknown keys still fail on both paths.
        assert!(get_config_val(&config, "bogus").is_err());
        assert!(set_config_val(&mut config, "bogus", "x").is_err());
    }

    #[test]
    fn test_edit_seam_success_and_editor_failure() {
        let _guard = EDIT_TEST_MUTEX.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("alx-config-edit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        let valid_toml = concat!(
            "[source]\n",
            "default_source = \"all\"\n",
            "default_quality = \"320k\"\n",
            "quality_fallback = [\"320k\"]\n",
            "js_priority = true\n",
            "priority = []\n",
            "\n[history]\n",
            "max_age_days = 90\n",
            "\n[display]\n",
            "color = \"auto\"\n",
            "table_style = \"rounded\"\n",
            "show_progress = true\n",
            "\n[network]\n",
            "timeout = 15\n",
            "max_retries = 2\n"
        );
        fs::write(&file, valid_toml).unwrap();

        // Successful editor exit over a valid file validates cleanly.
        run_edit_with(&file, "true", |program, path| {
            std::process::Command::new(program).arg(path).status()
        })
        .unwrap();

        // Editor exiting non-zero aborts with an error.
        let err = run_edit_with(&file, "false", |program, path| {
            std::process::Command::new(program).arg(path).status()
        })
        .unwrap_err();
        assert!(err.to_string().contains("unsuccessfully"));

        // Missing editor binary surfaces a launch error.
        let err = run_edit_with(&file, "alx-definitely-not-an-editor", |program, path| {
            std::process::Command::new(program).arg(path).status()
        })
        .unwrap_err();
        assert!(err.to_string().contains("Failed to launch editor"));

        // A successful editor exit over invalid TOML fails validation.
        fs::write(&file, "not [ valid toml ====").unwrap();
        let err = run_edit_with(&file, "true", |program, path| {
            std::process::Command::new(program).arg(path).status()
        })
        .unwrap_err();
        assert!(err.to_string().contains("invalid"));

        let _ = fs::remove_dir_all(&dir);
    }
}
