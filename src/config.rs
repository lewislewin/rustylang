use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub source_locale: String,
    pub file_pattern: String,
    pub locales: Vec<String>,
    pub concurrency: usize,
    pub openai: OpenAi,
    pub translate: Translate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAi {
    pub model: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Translate {
    pub overwrite_existing: bool,
    pub preserve_placeholders: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source_locale: "en-GB".to_string(),
            file_pattern: "{locale}.json".to_string(),
            locales: vec![],
            concurrency: 5,
            openai: OpenAi::default(),
            translate: Translate::default(),
        }
    }
}

impl Default for OpenAi {
    fn default() -> Self {
        Self { model: "gpt-4o-mini".to_string(), api_key: None }
    }
}

impl Default for Translate {
    fn default() -> Self {
        Self { overwrite_existing: false, preserve_placeholders: true }
    }
}

pub fn load_config() -> Result<Config> {
    let path = PathBuf::from("rustylang.toml");
    if !path.exists() {
        return Ok(Config::default());
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("Reading config file {:?}", path))?;
    let cfg: Config = toml::from_str(&contents)
        .with_context(|| format!("Parsing config file {:?}", path))?;
    Ok(cfg)
}



