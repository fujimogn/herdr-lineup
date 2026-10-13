//! Bounded plugin configuration loaded without writing user settings.

use crate::tokens::Agent;
use serde::Deserialize;
use std::{collections::HashSet, io, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub refresh_interval_ms: u64,
    pub reader_timeout_ms: u64,
    pub usage_max_age_ms: u64,
    pub logo_mode: String,
    pub non_first_pane_logo_indent: u8,
    pub providers: Vec<String>,
    /// Opt in to Codex app-server account queries and authenticated Grok billing.
    /// Local context/statusLine reads remain available when this is false.
    pub allow_network: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval_ms: 5000,
            reader_timeout_ms: 2000,
            usage_max_age_ms: 60000,
            logo_mode: "auto".into(),
            non_first_pane_logo_indent: 0,
            providers: Agent::ALL.iter().map(|entry| entry.id.to_owned()).collect(),
            allow_network: false,
        }
    }
}
impl Config {
    pub fn load(directory: &Path) -> io::Result<Self> {
        match std::fs::read_to_string(directory.join("config.toml")) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err(io::Error::other("cannot read plugin config.toml")),
        }
    }
    pub fn parse(text: &str) -> io::Result<Self> {
        // TOML errors can quote arbitrary user values; diagnostics never echo them.
        let config: Self = toml::from_str(text).map_err(|_| {
            io::Error::other(
                "invalid config.toml: malformed TOML, unknown key, or wrong value type",
            )
        })?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> io::Result<()> {
        let invalid = |message| Err(io::Error::other(message));
        if !(1000..=3600000).contains(&self.refresh_interval_ms) {
            return invalid("refresh_interval_ms must be 1000..=3600000");
        }
        if !(100..=30000).contains(&self.reader_timeout_ms)
            || self.reader_timeout_ms > self.refresh_interval_ms
        {
            return invalid(
                "reader_timeout_ms must be 100..=30000 and no greater than refresh_interval_ms",
            );
        }
        if self.usage_max_age_ms < self.refresh_interval_ms || self.usage_max_age_ms > 86400000 {
            return invalid(
                "usage_max_age_ms must be at least refresh_interval_ms and at most 86400000",
            );
        }
        if !["auto", "font", "text"].contains(&self.logo_mode.as_str()) {
            return invalid("logo_mode must be auto, font, or text");
        }
        if self.non_first_pane_logo_indent > 16 {
            return invalid("non_first_pane_logo_indent must be 0..=16");
        }
        let mut unique = HashSet::new();
        if self
            .providers
            .iter()
            .any(|p| Agent::from_id(p).is_none() || !unique.insert(p))
        {
            return invalid("providers must contain unique supported provider ids");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_indent_is_opt_in_and_bounded() {
        assert_eq!(Config::parse("").unwrap().non_first_pane_logo_indent, 0);
        for count in [0, 2, 16] {
            let config = Config::parse(&format!("non_first_pane_logo_indent = {count}")).unwrap();
            assert_eq!(config.non_first_pane_logo_indent, count);
        }
        for value in ["-1", "17", "1.5", "'2'", "true"] {
            assert!(Config::parse(&format!("non_first_pane_logo_indent = {value}")).is_err());
        }
    }

    #[test]
    fn absent_config_uses_defaults_without_creating_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.refresh_interval_ms, 5000);
        assert_eq!(
            config.providers,
            ["claude", "codex", "grok", "agy", "kimi", "hermes"]
        );
        assert!(!dir.path().join("config.toml").exists());
    }

    #[test]
    fn rejects_malformed_unknown_and_out_of_bounds_without_echoing_values() {
        for text in [
            "broken = [",
            "secret = 'PRIVATE'",
            "refresh_interval_ms = 999",
            "refresh_interval_ms = 1000",
            "reader_timeout_ms = 99",
            "usage_max_age_ms = 86400001",
            "providers = ['codex','codex']",
            "providers = ['unknown']",
            "logo_mode = 'invalid'",
        ] {
            let error = Config::parse(text).unwrap_err();
            assert!(!error.to_string().contains("PRIVATE"));
        }
    }

    #[test]
    fn accepts_bounds_and_empty_providers() {
        let config = Config::parse("refresh_interval_ms = 1000\nreader_timeout_ms = 100\nusage_max_age_ms = 1000\nproviders = []\nlogo_mode = 'text'").unwrap();
        assert!(config.providers.is_empty());
        assert!(Config::parse("refresh_interval_ms = 3600000\nreader_timeout_ms = 30000\nusage_max_age_ms = 86400000").is_ok());
    }
}
