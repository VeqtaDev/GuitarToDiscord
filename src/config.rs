use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Which channel of an input device to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelSel {
    /// Average all channels into mono.
    Mix,
    /// Use one channel (0-based).
    Index(u16),
}

impl Default for ChannelSel {
    fn default() -> Self {
        ChannelSel::Mix
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub host: Option<String>,
    pub guitar_device: Option<String>,
    pub guitar_channel: ChannelSel,
    pub mic_device: Option<String>,
    pub mic_channel: ChannelSel,
    pub output_device: Option<String>,
    pub monitor_device: Option<String>,
    pub monitor_enabled: bool,
    pub monitor_mode: crate::audio::MonitorMode,

    pub guitar_gain_db: f32,
    pub mic_gain_db: f32,
    pub master_gain_db: f32,
    pub monitor_gain_db: f32,
    pub guitar_gate_db: f32,
    pub mic_gate_db: f32,
    pub guitar_muted: bool,
    pub mic_muted: bool,

    pub vst_custom_dirs: Vec<PathBuf>,
    pub vst_chain: Vec<PathBuf>,
    pub auto_start: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: None,
            guitar_device: None,
            guitar_channel: ChannelSel::Index(0),
            mic_device: None,
            mic_channel: ChannelSel::Mix,
            output_device: None,
            monitor_device: None,
            monitor_enabled: false,
            monitor_mode: crate::audio::MonitorMode::Guitar,
            guitar_gain_db: 0.0,
            mic_gain_db: 0.0,
            master_gain_db: 0.0,
            monitor_gain_db: -6.0,
            guitar_gate_db: -96.0,
            mic_gate_db: -96.0,
            guitar_muted: false,
            mic_muted: false,
            vst_custom_dirs: Vec::new(),
            vst_chain: Vec::new(),
            auto_start: false,
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("GuitarToDiscord").join("config.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = std::fs::write(&p, s) {
                    log::warn!("could not save config: {e}");
                }
            }
            Err(e) => log::warn!("could not serialize config: {e}"),
        }
    }
}
