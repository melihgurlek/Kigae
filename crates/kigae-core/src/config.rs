//! `~/.config/kigae/config.toml` and `base.toml` (SPEC.md §2, §3.1).

use std::path::PathBuf;

use serde::Deserialize;

use crate::manifest::{Manifest, Unset, UnsetPolicy};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// The dotfiles repo with one branch per rice.
    pub repo: String,
    /// Branches to look at; a trailing `*` matches any rest.
    pub branches: Vec<String>,
    /// Manifests kept outside the rices' branches (default: `manifests/` next to this file).
    pub manifests: Option<String>,
    pub unset: UnsetConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            repo: "~/dotfiles".into(),
            branches: vec!["rice/*".into()],
            manifests: None,
            unset: UnsetConfig::default(),
        }
    }
}

/// What an app looks like when a rice doesn't theme it.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UnsetConfig {
    pub default: Unset,
    pub kconfig: Option<Unset>,
    pub links: Option<Unset>,
    pub vscode: Option<Unset>,
}

impl UnsetConfig {
    pub fn policy(&self) -> UnsetPolicy {
        UnsetPolicy {
            kconfig: self.kconfig.unwrap_or(self.default),
            links: self.links.unwrap_or(self.default),
            vscode: self.vscode.unwrap_or(self.default),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: {err}")]
    Read { path: PathBuf, err: std::io::Error },
    #[error("{path}: {err}")]
    Parse { path: PathBuf, err: String },
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("kigae")
}

fn read(path: &PathBuf) -> Result<Option<String>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ConfigError::Read { path: path.clone(), err }),
    }
}

impl Config {
    pub fn manifests_dir(&self) -> Option<String> {
        self.manifests.clone()
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = config_dir().join("config.toml");
        match read(&path)? {
            None => Ok(Config::default()),
            Some(t) => toml::from_str(&t).map_err(|e| ConfigError::Parse { path, err: e.to_string() }),
        }
    }
}

/// `base.toml`: written like a rice; fills in what a rice leaves out.
pub fn load_base() -> Result<Option<Manifest>, ConfigError> {
    let path = config_dir().join("base.toml");
    match read(&path)? {
        None => Ok(None),
        Some(t) => Manifest::parse(&t).map(Some).map_err(|e| ConfigError::Parse { path, err: e.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_section_overrides_default() {
        let c: Config = toml::from_str("[unset]\ndefault = \"stock\"\nvscode = \"keep\"").unwrap();
        let p = c.unset.policy();
        assert_eq!((p.kconfig, p.links, p.vscode), (Unset::Stock, Unset::Stock, Unset::Keep));
        assert_eq!(c.repo, "~/dotfiles");
        assert_eq!(c.manifests, None);
    }

    #[test]
    fn defaults_to_base() {
        let p = Config::default().unset.policy();
        assert_eq!((p.kconfig, p.links, p.vscode), (Unset::Base, Unset::Base, Unset::Base));
    }
}
