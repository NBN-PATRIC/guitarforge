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
    /// Quanto o gate espera aberto depois que a nota cai abaixo do limiar (nota longa não é cortada no meio).
    pub hold_ms: f32,
    /// Tempo para fechar depois que a nota some.
    pub release_ms: f32,
    /// Quanto o gate abaixa quando fecha (90 dB = mudo).
    pub range_db: f32,
}

impl Default for GateCfg {
    fn default() -> Self {
        // medido no cabo: zumbido parado entre -60 e -67 dBFS, conforme a mão e a chave
        Self { on: true, threshold_db: -52.0, hum_filter: true, auto: true, hold_ms: 50.0, release_ms: 80.0, range_db: 90.0 }
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

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EqCfg {
    pub on: bool,
    pub bands_db: [f32; 5],
    /// Corte de graves (passa-altas de 12 dB/oitava); 20 Hz = desligado.
    pub low_cut_hz: f32,
    /// Corte de agudos (passa-baixas de 12 dB/oitava); 20 kHz = desligado.
    pub high_cut_hz: f32,
}

impl Default for EqCfg {
    fn default() -> Self {
        Self { on: false, bands_db: [0.0; 5], low_cut_hz: 20.0, high_cut_hz: 20000.0 }
    }
}

/// Auto-wah: a palhetada abre o filtro.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WahCfg {
    pub on: bool,
    pub sens: f32,
    pub resonance: f32,
    pub range: f32,
}

impl Default for WahCfg {
    fn default() -> Self {
        Self { on: false, sens: 0.5, resonance: 0.5, range: 0.6 }
    }
}

/// Oitavador: oitava abaixo, oitava acima e o som direto.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OctaveCfg {
    pub on: bool,
    pub sub: f32,
    pub up: f32,
    pub dry: f32,
}

impl Default for OctaveCfg {
    fn default() -> Self {
        Self { on: false, sub: 0.6, up: 0.0, dry: 0.7 }
    }
}

/// Boost limpo (dB) com brilho.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BoostCfg {
    pub on: bool,
    pub level_db: f32,
    pub bright: f32,
}

impl Default for BoostCfg {
    fn default() -> Self {
        Self { on: false, level_db: 6.0, bright: 0.3 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PhaserCfg {
    pub on: bool,
    pub rate: f32,
    pub depth: f32,
    pub feedback: f32,
}

impl Default for PhaserCfg {
    fn default() -> Self {
        Self { on: false, rate: 0.3, depth: 0.7, feedback: 0.4 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FlangerCfg {
    pub on: bool,
    pub rate: f32,
    pub depth: f32,
    pub feedback: f32,
}

impl Default for FlangerCfg {
    fn default() -> Self {
        Self { on: false, rate: 0.2, depth: 0.6, feedback: 0.5 }
    }
}

/// Tremolo: `shape` vai da senoide (0) ao quase quadrado (1).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TremoloCfg {
    pub on: bool,
    pub rate: f32,
    pub depth: f32,
    pub shape: f32,
}

impl Default for TremoloCfg {
    fn default() -> Self {
        Self { on: false, rate: 0.4, depth: 0.5, shape: 0.0 }
    }
}

/// Estéreo por dobra (depois do delay, antes do reverb): `width` de 0 (mono) a 1.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SpreadCfg {
    pub on: bool,
    pub width: f32,
}

impl Default for SpreadCfg {
    fn default() -> Self {
        Self { on: false, width: 0.5 }
    }
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

/// De onde vem a ambiência: o reverb por algoritmo (leve, com o knob Sala), uma sala de fábrica por convolução
/// (IR gerado pelo app) ou o IR de sala de um arquivo.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ReverbKind {
    #[default]
    Algorithm,
    SmallRoom,
    Studio,
    Room,
    Hall,
    Church,
    File,
}

impl ReverbKind {
    /// As salas de fábrica (convolução com IR gerado), da menor para a maior.
    pub const ROOMS: [ReverbKind; 5] = [Self::SmallRoom, Self::Studio, Self::Room, Self::Hall, Self::Church];

    pub fn label(self) -> &'static str {
        match self {
            Self::Algorithm => "Algoritmo",
            Self::SmallRoom => "Quarto",
            Self::Studio => "Estúdio",
            Self::Room => "Sala",
            Self::Hall => "Salão",
            Self::Church => "Igreja",
            Self::File => "IR de arquivo",
        }
    }

    pub fn is_convolution(self) -> bool {
        self != Self::Algorithm
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ReverbCfg {
    pub on: bool,
    /// Tamanho do reverb por algoritmo.
    pub size: f32,
    pub mix: f32,
    pub kind: ReverbKind,
    /// Atraso antes da sala por convolução (0 a 150 ms).
    pub pre_delay_ms: f32,
}

impl Default for ReverbCfg {
    fn default() -> Self {
        Self { on: true, size: 0.5, mix: 0.2, kind: ReverbKind::Algorithm, pre_delay_ms: 0.0 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub input_gain_db: f32,
    pub master: f32,
    pub mute: bool,
    /// Compensação de volume do preset (volume igual entre presets), antes do volume geral.
    pub level_db: f32,
    pub gate: GateCfg,
    pub comp: CompCfg,
    pub wah: WahCfg,
    pub octave: OctaveCfg,
    pub boost: BoostCfg,
    pub drive: DriveCfg,
    pub chorus: ChorusCfg,
    pub phaser: PhaserCfg,
    pub flanger: FlangerCfg,
    pub tremolo: TremoloCfg,
    pub amp: AmpCfg,
    pub cab: CabCfg,
    pub eq: EqCfg,
    pub delay: DelayCfg,
    pub spread: SpreadCfg,
    pub reverb: ReverbCfg,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_gain_db: 0.0,
            master: 0.6,
            mute: false,
            level_db: 0.0,
            gate: GateCfg::default(),
            comp: CompCfg::default(),
            wah: WahCfg::default(),
            octave: OctaveCfg::default(),
            boost: BoostCfg::default(),
            drive: DriveCfg::default(),
            chorus: ChorusCfg::default(),
            phaser: PhaserCfg::default(),
            flanger: FlangerCfg::default(),
            tremolo: TremoloCfg::default(),
            amp: AmpCfg::default(),
            cab: CabCfg::default(),
            eq: EqCfg::default(),
            delay: DelayCfg::default(),
            spread: SpreadCfg::default(),
            reverb: ReverbCfg::default(),
        }
    }
}
