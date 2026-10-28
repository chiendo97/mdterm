use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct Config {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub line_numbers: bool,
    #[serde(default)]
    pub width: usize,
    #[serde(default = "default_mermaid_render")]
    pub mermaid_render: String,
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_mermaid_render() -> String {
    "auto".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            line_numbers: false,
            width: 0,
            mermaid_render: default_mermaid_render(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mermaid_render_is_auto() {
        assert_eq!(Config::default().mermaid_render, "auto");
    }

    #[test]
    fn parse_mermaid_render_config() {
        let config: Config = toml::from_str("mermaid_render = \"ascii\"\n").unwrap();
        assert_eq!(config.mermaid_render, "ascii");
    }
}
