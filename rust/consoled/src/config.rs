//! Daemon configuration (JSON, rendered by the NixOS module).

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    None,
    Rtscts,
    Xonxoff,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Port {
    /// Name clients use ("P12").
    pub name: String,
    /// Device node ("/dev/P12").
    pub device: PathBuf,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_baud")]
    pub baud: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_parity")]
    pub parity: Parity,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    #[serde(default = "default_flow")]
    pub flow: Flow,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Unix socket clients connect to.
    pub socket: PathBuf,
    /// Scrollback per port, bytes (kept in RAM only).
    #[serde(default = "default_scrollback")]
    pub scrollback: usize,
    pub ports: Vec<Port>,
}

fn default_baud() -> u32 {
    9600
}
fn default_data_bits() -> u8 {
    8
}
fn default_parity() -> Parity {
    Parity::None
}
fn default_stop_bits() -> u8 {
    1
}
fn default_flow() -> Flow {
    Flow::None
}
fn default_scrollback() -> usize {
    1 << 20
}

impl Port {
    /// "9600 8N1"
    pub fn line_settings(&self) -> String {
        let p = match self.parity {
            Parity::None => 'N',
            Parity::Even => 'E',
            Parity::Odd => 'O',
        };
        let f = match self.flow {
            Flow::None => "",
            Flow::Rtscts => " rtscts",
            Flow::Xonxoff => " xonxoff",
        };
        format!("{} {}{}{}{}", self.baud, self.data_bits, p, self.stop_bits, f)
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cfg: Config = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for p in &self.ports {
            if p.name.is_empty() || !p.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(format!("port name {:?}: use letters, digits, - and _", p.name));
            }
            if !seen.insert(p.name.to_ascii_lowercase()) {
                return Err(format!("port {} is configured twice", p.name));
            }
            if crate::serial::baud_constant(p.baud).is_none() {
                return Err(format!("port {}: unsupported baud rate {}", p.name, p.baud));
            }
            if !(5..=8).contains(&p.data_bits) || !(1..=2).contains(&p.stop_bits) {
                return Err(format!("port {}: data bits 5-8, stop bits 1-2", p.name));
            }
        }
        if self.scrollback == 0 {
            return Err("scrollback must be > 0".into());
        }
        Ok(())
    }

    /// Port index by name or label, case-insensitive.
    pub fn find(&self, key: &str) -> Option<usize> {
        let k = key.to_ascii_lowercase();
        self.ports
            .iter()
            .position(|p| p.name.to_ascii_lowercase() == k)
            .or_else(|| self.ports.iter().position(|p| !p.label.is_empty() && p.label.to_ascii_lowercase() == k))
    }
}
