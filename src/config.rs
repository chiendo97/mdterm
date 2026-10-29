use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

fn deserialize_categories<'de, D>(d: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Cat {
        List(Vec<String>),
        Single(String),
    }
    match Option::deserialize(d)? {
        Some(Cat::List(v)) => Ok(Some(v)),
        Some(Cat::Single(s)) => Ok(Some(vec![s])),
        None => Ok(None),
    }
}

#[derive(Deserialize)]
pub struct Config {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub line_numbers: bool,
    #[serde(default)]
    pub width: usize,
    #[serde(default)]
    pub pos: PosConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PosConfig {
    #[serde(default = "default_pos_enabled")]
    #[allow(dead_code)]
    pub enabled: bool,
    #[serde(default, deserialize_with = "deserialize_categories")]
    #[allow(dead_code)]
    pub categories: Option<Vec<String>>,
}

impl Default for PosConfig {
    fn default() -> Self {
        Self {
            enabled: default_pos_enabled(),
            categories: None,
        }
    }
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_pos_enabled() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            line_numbers: false,
            width: 0,
            pos: PosConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        if let Some(path) = config_path()
            && let Ok(contents) = fs::read_to_string(&path)
            && let Ok(config) = toml::from_str(&contents)
        {
            return config;
        }
        Config::default()
    }
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(dirs::config_dir)?;
    Some(base.join("mdterm").join("config.toml"))
}
