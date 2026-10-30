use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::dsp::tuner::{self, Notation, Tuning};
use crate::paths;
use crate::settings::{AmpChannel, CabKind, DriveKind, Settings};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    pub settings: Settings,
    pub ir_path: Option<String>,
    /// Afinação do preset (id de `Tuning::id`): carregar o som de uma música já troca o afinador.
    pub tuning: Option<String>,
    #[serde(skip)]
    pub factory: bool,
}

impl Default for Preset {
    fn default() -> Self {
        Self { name: "Sem nome".into(), settings: Settings::default(), ir_path: None, tuning: None, factory: false }
    }
}

impl Preset {
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).with_context(|| format!("gravando {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path).with_context(|| format!("lendo {}", path.display()))?;
        Ok(serde_json::from_str(&data)?)
    }
}

fn make(name: &str, edit: impl FnOnce(&mut Settings)) -> Preset {
    let mut s = Settings::default();
    edit(&mut s);
    Preset { name: name.into(), settings: s, ir_path: None, tuning: None, factory: true }
}

pub fn factory() -> Vec<Preset> {
    vec![
        make("Limpo cristalino", |s| {
            s.amp.gain = 0.35;
            s.amp.mid = 0.55;
            s.amp.treble = 0.6;
            s.comp.on = true;
            s.comp.sustain = 0.4;
            s.chorus.on = true;
            s.chorus.rate = 0.25;
            s.chorus.depth = 0.4;
            s.chorus.mix = 0.35;
            s.reverb.size = 0.55;
            s.reverb.mix = 0.25;
        }),
        make("Blues", |s| {
            s.amp.channel = AmpChannel::Crunch;
            s.amp.gain = 0.35;
            s.amp.bass = 0.55;
            s.amp.mid = 0.65;
            s.drive.on = true;
            s.drive.kind = DriveKind::Overdrive;
            s.drive.gain = 0.3;
            s.drive.level = 0.55;
            s.cab.kind = CabKind::Open2x12;
            s.reverb.size = 0.45;
        }),
        make("Crunch clássico", |s| {
            s.amp.channel = AmpChannel::Crunch;
            s.amp.gain = 0.6;
            s.amp.bass = 0.55;
            s.amp.mid = 0.6;
            s.amp.treble = 0.6;
            s.amp.presence = 0.5;
            s.cab.kind = CabKind::Closed4x12;
            s.reverb.size = 0.3;
            s.reverb.mix = 0.15;
        }),
        make("Rock", |s| {
            s.amp.channel = AmpChannel::Lead;
            s.amp.gain = 0.5;
            s.amp.bass = 0.6;
            s.amp.treble = 0.6;
            s.amp.presence = 0.55;
            s.cab.kind = CabKind::Closed4x12;
            s.reverb.size = 0.3;
            s.reverb.mix = 0.12;
        }),
        make("Solo com delay", |s| {
            s.amp.channel = AmpChannel::Lead;
            s.amp.gain = 0.7;
            s.amp.mid = 0.7;
            s.amp.treble = 0.55;
            s.cab.kind = CabKind::Closed4x12;
            s.delay.on = true;
            s.reverb.size = 0.5;
            s.reverb.mix = 0.22;
        }),
        make("Metal", |s| {
            s.gate.threshold_db = -48.0;
            s.amp.channel = AmpChannel::Lead;
            s.amp.gain = 0.85;
            s.amp.bass = 0.65;
            s.amp.mid = 0.35;
            s.amp.treble = 0.65;
            s.amp.presence = 0.6;
            s.drive.on = true;
            s.drive.gain = 0.1;
            s.drive.tone = 0.6;
            s.drive.level = 0.65;
            s.cab.kind = CabKind::Closed4x12;
            s.reverb.size = 0.25;
            s.reverb.mix = 0.1;
        }),
        make("Ambiente", |s| {
            s.amp.gain = 0.3;
            s.amp.treble = 0.6;
            s.chorus.on = true;
            s.chorus.rate = 0.2;
            s.chorus.depth = 0.6;
            s.chorus.mix = 0.45;
            s.delay.on = true;
            s.delay.time_ms = 520.0;
            s.delay.feedback = 0.5;
            s.delay.mix = 0.35;
            s.reverb.size = 0.85;
            s.reverb.mix = 0.4;
        }),
        make("Direto (só o cabo)", |s| {
            s.gate.on = false;
            s.amp.on = false;
            s.cab.on = false;
            s.reverb.on = false;
        }),
    ]
}

pub fn user_presets() -> Vec<Preset> {
    let mut list: Vec<Preset> = std::fs::read_dir(paths::presets_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
        .filter_map(|p| match Preset::load(&p) {
            Ok(preset) => Some(preset),
            Err(e) => {
                log::warn!("preset ignorado ({}): {e:#}", p.display());
                None
            }
        })
        .collect();
    list.sort_by_key(|p| p.name.to_lowercase());
    list
}

pub fn user_preset_path(name: &str) -> PathBuf {
    let clean: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' })
        .collect();
    paths::presets_dir().join(format!("{clean}.json"))
}

/// Afinações criadas pelo usuário, em `%APPDATA%\GuitarForge\afinacoes.json`.
pub fn custom_tunings() -> Vec<Tuning> {
    let path = paths::tunings_file();
    let Ok(data) = std::fs::read_to_string(&path) else { return Vec::new() };
    match serde_json::from_str::<Vec<Tuning>>(&data) {
        Ok(mut list) => {
            list.retain(|t| (tuner::MIN_STRINGS..=tuner::MAX_STRINGS).contains(&t.midi.len()));
            for t in &mut list {
                t.custom = true;
            }
            list
        }
        Err(e) => {
            log::warn!("afinações ilegíveis em {}: {e}", path.display());
            Vec::new()
        }
    }
}

pub fn save_custom_tunings(list: &[Tuning]) -> Result<()> {
    let path = paths::tunings_file();
    let json = serde_json::to_string_pretty(list)?;
    std::fs::write(&path, json).with_context(|| format!("gravando {}", path.display()))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum LatencyMode {
    Low,
    #[default]
    Normal,
    Safe,
}

impl LatencyMode {
    pub const ALL: [LatencyMode; 3] = [Self::Low, Self::Normal, Self::Safe];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Baixa (pode estalar)",
            Self::Normal => "Normal",
            Self::Safe => "Segura (mais atraso)",
        }
    }

    /// Folga no anel entre entrada e saída, em blocos de entrada.
    pub fn margin_blocks(self) -> f32 {
        match self {
            Self::Low => 0.5,
            Self::Normal => 1.0,
            Self::Safe => 2.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub preset: Preset,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub latency: LatencyMode,
    pub tuning: String,
    pub notation: Notation,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            preset: factory().remove(0),
            input_device: None,
            output_device: None,
            latency: LatencyMode::Normal,
            tuning: Tuning::default().id(),
            notation: Notation::Solfege,
        }
    }
}

impl AppState {
    pub fn load() -> Self {
        let path = paths::state_file();
        match std::fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_else(|e| {
                log::warn!("estado ilegível, usando o padrão: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = paths::state_file();
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    log::warn!("não consegui gravar {}: {e}", path.display());
                }
            }
            Err(e) => log::warn!("estado não serializável: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_defaults() {
        let p: Preset = serde_json::from_str(r#"{"name":"x","settings":{"amp":{"gain":0.9}}}"#).unwrap();
        assert_eq!(p.settings.amp.gain, 0.9);
        assert_eq!(p.settings.amp.bass, 0.5);
        assert!(p.settings.gate.on);
    }

    #[test]
    fn preset_roundtrip() {
        for p in factory() {
            let json = serde_json::to_string(&p).unwrap();
            let back: Preset = serde_json::from_str(&json).unwrap();
            assert_eq!(back.settings, p.settings);
            assert_eq!(back.name, p.name);
        }
    }

    #[test]
    fn preset_file_name_is_sanitized() {
        let p = user_preset_path("Meu/som: teste?");
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "Meu_som_ teste_.json");
    }
}
