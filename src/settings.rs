use serde::{Deserialize, Serialize};

pub const EQ_FREQS: [f32; 5] = [100.0, 250.0, 800.0, 2500.0, 6000.0];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum DriveKind {
    #[default]
    Overdrive,
    Distortion,
    Fuzz,
}

impl DriveKind {
    pub const ALL: [DriveKind; 3] = [Self::Overdrive, Self::Distortion, Self::Fuzz];

    pub fn label(self) -> &'static str {
        match self {
            Self::Overdrive => "Overdrive",
            Self::Distortion => "Distorção",
            Self::Fuzz => "Fuzz",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum AmpChannel {
    #[default]
    Clean,
    Crunch,
    Lead,
}

impl AmpChannel {
    pub const ALL: [AmpChannel; 3] = [Self::Clean, Self::Crunch, Self::Lead];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clean => "LIMPO",
            Self::Crunch => "CRUNCH",
            Self::Lead => "LEAD",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum CabKind {
    #[default]
    Combo1x12,
    Open2x12,
    Closed4x12,
}

impl CabKind {
    pub const ALL: [CabKind; 3] = [Self::Combo1x12, Self::Open2x12, Self::Closed4x12];

    pub fn label(self) -> &'static str {
        match self {
            Self::Combo1x12 => "Combo 1x12",
            Self::Open2x12 => "Aberto 2x12",
            Self::Closed4x12 => "Fechado 4x12",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GateCfg {
    pub on: bool,
    pub threshold_db: f32,
    /// Filtro de zumbido da rede (60 Hz e harmônicos) antes do gate.
    pub hum_filter: bool,
    /// Limiar ajustado sozinho pelo ruído de fundo medido (o `threshold_db` só vale com isso desligado).
    pub auto: bool,
}

impl Default for GateCfg {
    fn default() -> Self {
        // medido no cabo: zumbido parado entre -60 e -67 dBFS, conforme a mão e a chave
        Self { on: true, threshold_db: -52.0, hum_filter: true, auto: true }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CompCfg {
    pub on: bool,
    pub sustain: f32,
    pub level: f32,
}

impl Default for CompCfg {
    fn default() -> Self {
        Self { on: false, sustain: 0.5, level: 0.5 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DriveCfg {
    pub on: bool,
    pub kind: DriveKind,
    pub gain: f32,
    pub tone: f32,
    pub level: f32,
}

impl Default for DriveCfg {
    fn default() -> Self {
        Self { on: false, kind: DriveKind::Overdrive, gain: 0.4, tone: 0.5, level: 0.5 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ChorusCfg {
    pub on: bool,
    pub rate: f32,
    pub depth: f32,
    pub mix: f32,
}

impl Default for ChorusCfg {
    fn default() -> Self {
        Self { on: false, rate: 0.3, depth: 0.5, mix: 0.5 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AmpCfg {
    pub on: bool,
    pub channel: AmpChannel,
    pub gain: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub presence: f32,
    pub volume: f32,
}

impl Default for AmpCfg {
    fn default() -> Self {
        Self {
            on: true,
            channel: AmpChannel::Clean,
            gain: 0.4,
            bass: 0.5,
            mid: 0.5,
            treble: 0.5,
            presence: 0.4,
            volume: 0.6,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CabCfg {
    pub on: bool,
    pub kind: CabKind,
    pub use_ir: bool,
}

impl Default for CabCfg {
    fn default() -> Self {
        Self { on: true, kind: CabKind::Combo1x12, use_ir: false }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EqCfg {
    pub on: bool,
    pub bands_db: [f32; 5],
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayCfg {
    pub on: bool,
    pub time_ms: f32,
    pub feedback: f32,
    pub mix: f32,
}

impl Default for DelayCfg {
    fn default() -> Self {
        Self { on: false, time_ms: 380.0, feedback: 0.35, mix: 0.3 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ReverbCfg {
    pub on: bool,
    pub size: f32,
    pub mix: f32,
}

impl Default for ReverbCfg {
    fn default() -> Self {
        Self { on: true, size: 0.5, mix: 0.2 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub input_gain_db: f32,
    pub master: f32,
    pub mute: bool,
    pub gate: GateCfg,
    pub comp: CompCfg,
    pub drive: DriveCfg,
    pub chorus: ChorusCfg,
    pub amp: AmpCfg,
    pub cab: CabCfg,
    pub eq: EqCfg,
    pub delay: DelayCfg,
    pub reverb: ReverbCfg,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_gain_db: 0.0,
            master: 0.6,
            mute: false,
            gate: GateCfg::default(),
            comp: CompCfg::default(),
            drive: DriveCfg::default(),
            chorus: ChorusCfg::default(),
            amp: AmpCfg::default(),
            cab: CabCfg::default(),
            eq: EqCfg::default(),
            delay: DelayCfg::default(),
            reverb: ReverbCfg::default(),
        }
    }
}
