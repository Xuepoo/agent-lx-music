#![allow(clippy::manual_strip, clippy::collapsible_if)]
use crate::library::db::{SourceDbEntry, insert_or_update_source};
use crate::source::runtime::JsSandbox;
use anyhow::{Result, anyhow};
use md5::Digest;
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone)]
pub struct SourceMetadata {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
}

pub fn parse_metadata(script_content: &str) -> Result<SourceMetadata> {
    let start_idx = script_content
        .find("/*")
        .ok_or_else(|| anyhow!("Invalid script: Missing metadata comment"))?;
    let end_idx = script_content[start_idx..]
        .find("*/")
        .map(|idx| idx + start_idx)
        .ok_or_else(|| anyhow!("Invalid script: Unterminated metadata comment"))?;

    let comment_body = &script_content[start_idx + 2..end_idx];
    let mut fields = HashMap::new();

    for line in comment_body.lines() {
        let trimmed = line.trim().trim_start_matches('*').trim();
        if trimmed.starts_with('@') {
            let mut parts = trimmed[1..].splitn(2, ' ');
            if let Some(key) = parts.next() {
                let val = parts.next().unwrap_or("").trim().to_string();
                fields.insert(key.to_string(), val);
            }
        }
    }

    let name = fields
        .get("name")
        .cloned()
        .ok_or_else(|| anyhow!("Invalid script: Missing '@name' field in metadata"))?;

    Ok(SourceMetadata {
        name,
        description: fields.get("description").cloned(),
        version: fields.get("version").cloned(),
        author: fields.get("author").cloned(),
        homepage: fields.get("homepage").cloned(),
        repository: fields
            .get("repository")
            .cloned()
            .or_else(|| fields.get("homepage").cloned()),
    })
}

pub fn add_source_script(path_or_url: &str) -> Result<()> {
    // 1. Load script content
    let content = if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let client = crate::cmd::download::http_client_builder(
                    crate::cmd::download::http_timeout(None),
                )
                .build()?;
                let resp = client.get(path_or_url).send().await?;
                resp.text().await
            })
        })?
    } else {
        fs::read_to_string(path_or_url)
            .map_err(|e| anyhow!("Failed to read local script: {}", e))?
    };

    // 2. Parse Metadata comment
    let meta = parse_metadata(&content)?;

    // 3. Compute hash
    let mut hasher = md5::Md5::new();
    hasher.update(content.as_bytes());
    let hash = hex::encode(hasher.finalize());

    // 4. Validate and capture 'inited' event via JsSandbox
    let sandbox = JsSandbox::new()?;
    let inited_val = sandbox.execute_init(&content)?;

    let sources_obj = inited_val
        .get("sources")
        .ok_or_else(|| anyhow!("Script registered successfully but missed 'sources' mapping"))?;

    // Parse platforms list and qualities map
    let mut platforms = Vec::new();
    let mut qualities_map = HashMap::new();

    if let Some(obj) = sources_obj.as_object() {
        for (platform_key, val) in obj {
            platforms.push(platform_key.clone());
            if let Some(plat_meta) = val.as_object() {
                if let Some(quals) = plat_meta.get("qualitys") {
                    qualities_map.insert(platform_key.clone(), quals.clone());
                }
            }
        }
    }

    let platforms_json = serde_json::to_string(&platforms)?;
    let qualities_json = serde_json::to_string(&qualities_map)?;

    // 5. Build ID and target XDG script path
    // ID = clean name in lower ascii + optional version
    let clean_name: String = meta
        .name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let id = if let Some(ref v) = meta.version {
        let clean_version: String = v
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
            .collect();
        format!("{}_{}", clean_name.to_lowercase(), clean_version)
    } else {
        clean_name.to_lowercase()
    };

    let paths = lux_core::config::resolve_paths();
    fs::create_dir_all(&paths.sources_dir)?;
    let script_path = paths.sources_dir.join(format!("{}.js", id));
    fs::write(&script_path, &content)?;

    // 6. Insert into database
    let now = chrono::Utc::now().to_rfc3339();
    let entry = SourceDbEntry {
        id,
        name: meta.name,
        version: meta.version,
        author: meta.author,
        homepage: meta.homepage,
        repository: meta.repository,
        script_path: script_path.to_string_lossy().to_string(),
        source_url: if path_or_url.starts_with("http") {
            Some(path_or_url.to_string())
        } else {
            None
        },
        content_hash: hash,
        platforms: platforms_json,
        qualities: qualities_json,
        enabled: true,
        created_at: now.clone(),
        updated_at: now,
    };

    insert_or_update_source(&entry)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_metadata() {
        let script = r#"
/*!
 * @name Test Source
 * @description v1.0.0 - Test
 * @version v1.0.0
 * @author Tester
 * @homepage https://example.com
 */
console.log("hello");
"#;
        let meta = parse_metadata(script).unwrap();
        assert_eq!(meta.name, "Test Source");
        assert_eq!(meta.version.as_deref(), Some("v1.0.0"));
        assert_eq!(meta.author.as_deref(), Some("Tester"));
        assert_eq!(meta.homepage.as_deref(), Some("https://example.com"));
        assert_eq!(meta.repository.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn test_sandbox_execute_init() {
        let script = r#"
        const { send } = globalThis.lx;
        send('inited', {
            status: true,
            sources: {
                kw: { type: 'music', qualitys: ['128k', '320k'] }
            }
        });
        "#;
        let sandbox = JsSandbox::new().unwrap();
        let val = sandbox.execute_init(script).unwrap();
        assert_eq!(
            val.pointer("/sources/kw/type").unwrap().as_str(),
            Some("music")
        );
    }

    #[test]
    fn test_sandbox_execute_resolve_promise() {
        let script = r#"
        const { on } = globalThis.lx;
        on('request', async ({ action, source, info }) => {
            return "https://music.download.url/song.mp3";
        });
        "#;
        let sandbox = JsSandbox::new().unwrap();
        let url = sandbox
            .execute_resolve(script, "kw", "12345", "320k", serde_json::json!({}))
            .unwrap();
        assert_eq!(url, "https://music.download.url/song.mp3");
    }
}
