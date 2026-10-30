mod convolver;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub use convolver::Convolver;

use crate::dsp::biquad::Biquad;
use crate::settings::{CabCfg, CabKind};

/// Até 170 ms a 48 kHz: cobre os IRs de gabinete comuns.
pub const MAX_IR_LEN: usize = 8192;

/// Gabinete: voicings internos feitos de filtros (sempre disponíveis) ou um IR carregado de WAV.
pub struct CabinetSim {
    sr: f32,
    kind: Option<CabKind>,
    filters: [Biquad; 5],
    conv: Option<Box<Convolver>>,
    use_ir: bool,
}

impl CabinetSim {
    pub fn new(sr: f32) -> Self {
        let mut c = Self { sr, kind: None, filters: [Biquad::passthrough(); 5], conv: None, use_ir: false };
        c.set(&CabCfg::default());
        c
    }

    pub fn set(&mut self, c: &CabCfg) {
        self.use_ir = c.use_ir;
        if self.kind != Some(c.kind) {
            self.kind = Some(c.kind);
            self.filters = voicing(self.sr, c.kind);
        }
    }

    pub fn swap_convolver(&mut self, conv: Option<Box<Convolver>>) -> Option<Box<Convolver>> {
        std::mem::replace(&mut self.conv, conv)
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        match (&mut self.conv, self.use_ir) {
            (Some(conv), true) => conv.process(buf),
            _ => {
                for s in buf.iter_mut() {
                    *s = self.filters.iter_mut().fold(*s, |x, f| f.tick(x));
                }
            }
        }
    }
}

fn voicing(sr: f32, kind: CabKind) -> [Biquad; 5] {
    match kind {
        CabKind::Combo1x12 => [
            Biquad::highpass(sr, 85.0, 0.7),
            Biquad::peaking(sr, 120.0, 1.2, 2.0),
            Biquad::peaking(sr, 2300.0, 1.4, 3.0),
            Biquad::lowpass(sr, 5200.0, 0.7),
            Biquad::lowpass(sr, 7000.0, 0.5),
        ],
        CabKind::Open2x12 => [
            Biquad::highpass(sr, 70.0, 0.7),
            Biquad::peaking(sr, 110.0, 1.0, 2.5),
            Biquad::peaking(sr, 600.0, 1.0, -1.5),
            Biquad::peaking(sr, 1800.0, 1.2, 2.5),
            Biquad::lowpass(sr, 4800.0, 0.7),
        ],
        CabKind::Closed4x12 => [
            Biquad::highpass(sr, 65.0, 0.8),
            Biquad::peaking(sr, 105.0, 1.4, 4.0),
            Biquad::peaking(sr, 450.0, 1.0, -2.5),
            Biquad::peaking(sr, 2700.0, 2.0, 4.5),
            Biquad::lowpass(sr, 4300.0, 0.8),
        ],
    }
}

/// IR de gabinete lido de um WAV (qualquer taxa, 16/24/32 bits ou float; usa o 1º canal).
pub struct ImpulseResponse {
    pub name: String,
    pub path: PathBuf,
    samples: Vec<f32>,
    rate: u32,
}

impl ImpulseResponse {
    pub fn load(path: &Path) -> Result<Self> {
        let mut reader = hound::WavReader::open(path).with_context(|| format!("abrindo {}", path.display()))?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;
        let raw: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(8, 32) - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<Result<_, _>>()?
            }
        };
        let samples: Vec<f32> = raw.chunks(channels).map(|frame| frame[0]).collect();
        if samples.is_empty() {
            bail!("o arquivo não tem áudio");
        }
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Self { name, path: path.to_path_buf(), samples, rate: spec.sample_rate })
    }

    /// IR na taxa do motor, cortado em `MAX_IR_LEN`, com fade no fim e energia normalizada.
    pub fn prepare(&self, rate: u32) -> Vec<f32> {
        let mut ir = if self.rate == rate {
            self.samples.clone()
        } else {
            resample_linear(&self.samples, self.rate as f64 / rate as f64)
        };
        ir.truncate(MAX_IR_LEN);
        let fade = ir.len().min(64);
        let start = ir.len() - fade;
        for (i, s) in ir[start..].iter_mut().enumerate() {
            *s *= 1.0 - i as f32 / fade as f32;
        }
        let energy: f32 = ir.iter().map(|x| x * x).sum();
        if energy > 1e-12 {
            let gain = 1.0 / energy.sqrt();
            ir.iter_mut().for_each(|x| *x *= gain);
        }
        ir
    }

    pub fn convolver(&self, rate: u32) -> Box<Convolver> {
        Box::new(Convolver::new(&self.prepare(rate)))
    }
}

fn resample_linear(input: &[f32], step: f64) -> Vec<f32> {
    let out_len = ((input.len() as f64) / step).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let i0 = pos as usize;
            let frac = (pos - i0 as f64) as f32;
            let a = input[i0];
            let b = *input.get(i0 + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_24bit_wav_and_prepares_for_engine_rate() {
        let path = std::env::temp_dir().join("guitarforge-teste-ir.wav");
        let spec = hound::WavSpec { channels: 2, sample_rate: 44100, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..2000 {
            let v = ((1 << 22) as f32 * (-(i as f32) / 200.0).exp()) as i32;
            w.write_sample(v).unwrap();
            w.write_sample(0).unwrap();
        }
        w.finalize().unwrap();

        let ir = ImpulseResponse::load(&path).unwrap();
        let prepared = ir.prepare(48000);
        let _ = std::fs::remove_file(&path);
        assert!((prepared.len() as i64 - 2176).abs() <= 2, "len {}", prepared.len());
        let energy: f32 = prepared.iter().map(|x| x * x).sum();
        assert!((energy - 1.0).abs() < 1e-3);
        assert!(prepared[0] > prepared[500]);
    }

    #[test]
    fn builtin_voicings_are_near_unity_in_the_mids() {
        for kind in CabKind::ALL {
            let mut cab = CabinetSim::new(48000.0);
            cab.set(&CabCfg { on: true, kind, use_ir: false });
            let chain = cab.filters;
            let mut gain = 1.0;
            for f in chain {
                gain *= crate::dsp::biquad::steady_gain(f, 48000.0, 1000.0);
            }
            let db = 20.0 * gain.log10();
            assert!(db.abs() < 4.0, "{kind:?}: {db} dB");
        }
    }
}
