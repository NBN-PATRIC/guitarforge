pub mod amp;
pub mod biquad;
pub mod cabinet;
pub mod effects;
pub mod hum_filter;
pub mod hum_meter;
pub mod noise_gate;
pub mod oversample;
pub mod tuner;

use crate::settings::Settings;

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Coeficiente de um filtro de um polo com constante de tempo `ms`.
pub fn time_coef(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms * 0.001 * sr)).exp()
}

struct Smoothed {
    current: f32,
    target: f32,
    coef: f32,
}

impl Smoothed {
    fn new(value: f32, sr: f32) -> Self {
        Self { current: value, target: value, coef: 1.0 - time_coef(20.0, sr) }
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coef;
        self.current
    }
}

/// Cadeia fixa: entrada > filtro de 60 Hz > gate > comp > wah > oitava > boost > drive > chorus > phaser > flanger >
/// tremolo > amp > gabinete > EQ > delay > reverb > volume.
pub struct SignalChain {
    input_gain: Smoothed,
    master: Smoothed,
    hum_on: bool,
    hum: hum_filter::HumFilter,
    floor: noise_gate::FloorTracker,
    auto_gate: bool,
    manual_threshold_db: f32,
    /// Limiar em uso pelo gate automático (para mostrar no knob).
    pub auto_threshold_db: Option<f32>,
    sample_rate: f32,
    gate_on: bool,
    gate: noise_gate::NoiseGate,
    comp_on: bool,
    comp: effects::Compressor,
    wah_on: bool,
    wah: effects::Wah,
    octave_on: bool,
    octave: effects::Octave,
    boost_on: bool,
    boost: effects::Boost,
    drive_on: bool,
    drive: effects::Drive,
    drive_os: oversample::Oversampler,
    chorus_on: bool,
    chorus: effects::Chorus,
    phaser_on: bool,
    phaser: effects::Phaser,
    flanger_on: bool,
    flanger: effects::Flanger,
    tremolo_on: bool,
    tremolo: effects::Tremolo,
    amp_on: bool,
    amp: amp::AmpSim,
    amp_os: oversample::Oversampler,
    cab_on: bool,
    pub cabinet: cabinet::CabinetSim,
    eq_on: bool,
    eq: effects::ParametricEq,
    delay_on: bool,
    delay: effects::Delay,
    reverb_on: bool,
    reverb: effects::Reverb,
}

impl SignalChain {
    /// `oversample`: 1, 2 ou 4; o drive e o amp rodam nessa taxa multiplicada.
    pub fn new(sr: f32, settings: &Settings, oversample: usize) -> Self {
        let hi = sr * oversample.max(1) as f32;
        let mut chain = Self {
            input_gain: Smoothed::new(1.0, sr),
            master: Smoothed::new(0.0, sr),
            hum_on: false,
            hum: hum_filter::HumFilter::new(sr, hum_filter::DEFAULT_MAINS_HZ),
            floor: noise_gate::FloorTracker::new(sr),
            auto_gate: true,
            manual_threshold_db: -52.0,
            auto_threshold_db: None,
            sample_rate: sr,
            gate_on: false,
            gate: noise_gate::NoiseGate::new(sr),
            comp_on: false,
            comp: effects::Compressor::new(sr),
            wah_on: false,
            wah: effects::Wah::new(sr),
            octave_on: false,
            octave: effects::Octave::new(sr),
            boost_on: false,
            boost: effects::Boost::new(sr),
            drive_on: false,
            drive: effects::Drive::new(hi),
            drive_os: oversample::Oversampler::new(oversample),
            chorus_on: false,
            chorus: effects::Chorus::new(sr),
            phaser_on: false,
            phaser: effects::Phaser::new(sr),
            flanger_on: false,
            flanger: effects::Flanger::new(sr),
            tremolo_on: false,
            tremolo: effects::Tremolo::new(sr),
            amp_on: false,
            amp: amp::AmpSim::new(hi),
            amp_os: oversample::Oversampler::new(oversample),
            cab_on: false,
            cabinet: cabinet::CabinetSim::new(sr),
            eq_on: false,
            eq: effects::ParametricEq::new(sr),
            delay_on: false,
            delay: effects::Delay::new(sr),
            reverb_on: false,
            reverb: effects::Reverb::new(sr),
        };
        chain.apply(settings);
        chain
    }

    pub fn apply(&mut self, s: &Settings) {
        self.input_gain.target = db_to_lin(s.input_gain_db);
        self.master.target = if s.mute { 0.0 } else { s.master * s.master };

        self.hum_on = s.gate.hum_filter;
        self.gate_on = s.gate.on;
        self.auto_gate = s.gate.auto;
        self.manual_threshold_db = s.gate.threshold_db;
        self.gate.set_times(s.gate.hold_ms, s.gate.release_ms, s.gate.range_db);
        if !self.auto_gate {
            self.gate.set_threshold_db(s.gate.threshold_db);
        }
        self.comp_on = s.comp.on;
        self.comp.set(s.comp.sustain, s.comp.level);
        if s.wah.on && !self.wah_on {
            self.wah.clear();
        }
        self.wah_on = s.wah.on;
        self.wah.set(s.wah.sens, s.wah.resonance, s.wah.range);
        if s.octave.on && !self.octave_on {
            self.octave.clear();
        }
        self.octave_on = s.octave.on;
        self.octave.set(s.octave.sub, s.octave.up, s.octave.dry);
        self.boost_on = s.boost.on;
        self.boost.set(s.boost.level_db, s.boost.bright);
        self.drive_on = s.drive.on;
        self.drive.set(&s.drive);

        if s.chorus.on && !self.chorus_on {
            self.chorus.clear();
        }
        self.chorus_on = s.chorus.on;
        self.chorus.set(s.chorus.rate, s.chorus.depth, s.chorus.mix);
        if s.phaser.on && !self.phaser_on {
            self.phaser.clear();
        }
        self.phaser_on = s.phaser.on;
        self.phaser.set(s.phaser.rate, s.phaser.depth, s.phaser.feedback);
        if s.flanger.on && !self.flanger_on {
            self.flanger.clear();
        }
        self.flanger_on = s.flanger.on;
        self.flanger.set(s.flanger.rate, s.flanger.depth, s.flanger.feedback);
        self.tremolo_on = s.tremolo.on;
        self.tremolo.set(s.tremolo.rate, s.tremolo.depth, s.tremolo.shape);

        self.amp_on = s.amp.on;
        self.amp.set(&s.amp);
        self.cab_on = s.cab.on;
        self.cabinet.set(&s.cab);
        self.eq_on = s.eq.on;
        self.eq.set(&s.eq.bands_db, s.eq.low_cut_hz, s.eq.high_cut_hz);

        self.delay.set(s.delay.time_ms, s.delay.feedback, s.delay.mix);
        if s.delay.on && !self.delay_on {
            self.delay.clear();
        }
        self.delay_on = s.delay.on;

        if s.reverb.on && !self.reverb_on {
            self.reverb.clear();
        }
        self.reverb_on = s.reverb.on;
        self.reverb.set(s.reverb.size, s.reverb.mix);
    }

    pub fn set_mains(&mut self, hz: f32) {
        self.hum.retune(hz);
    }

    pub fn gate_open(&self) -> bool {
        !self.gate_on || self.gate.open
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for x in buf.iter_mut() {
            *x *= self.input_gain.tick();
        }
        if self.hum_on {
            self.hum.process(buf);
        }
        self.floor.push(buf);
        if let Some(target) = self.floor.floor_db().map(noise_gate::auto_threshold_db) {
            // desce na hora (ruído caiu), sobe no máximo 3 dB/s: nada muda de repente no meio de uma nota
            let max_rise = 3.0 * buf.len() as f32 / self.sample_rate;
            self.auto_threshold_db = Some(match self.auto_threshold_db {
                Some(cur) if target > cur => (cur + max_rise).min(target),
                _ => target,
            });
        }
        if self.auto_gate {
            self.gate.set_threshold_db(self.auto_threshold_db.unwrap_or(self.manual_threshold_db));
        }
        if self.gate_on {
            self.gate.process(buf);
        }
        if self.comp_on {
            self.comp.process(buf);
        }
        if self.wah_on {
            self.wah.process(buf);
        }
        if self.octave_on {
            self.octave.process(buf);
        }
        if self.boost_on {
            self.boost.process(buf);
        }
        if self.drive_on {
            let drive = &mut self.drive;
            self.drive_os.process(buf, |hi| drive.process(hi));
        }
        if self.chorus_on {
            self.chorus.process(buf);
        }
        if self.phaser_on {
            self.phaser.process(buf);
        }
        if self.flanger_on {
            self.flanger.process(buf);
        }
        if self.tremolo_on {
            self.tremolo.process(buf);
        }
        if self.amp_on {
            let amp = &mut self.amp;
            self.amp_os.process(buf, |hi| amp.process(hi));
        }
        if self.cab_on {
            self.cabinet.process(buf);
        }
        if self.eq_on {
            self.eq.process(buf);
        }
        if self.delay_on {
            self.delay.process(buf);
        }
        if self.reverb_on {
            self.reverb.process(buf);
        }
        for x in buf.iter_mut() {
            *x = soft_limit(*x * self.master.tick());
        }
    }
}

/// Linear até 0,7 e encosta suavemente em 1,0: nunca clipa duro na placa de som.
fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.7;
    let a = x.abs();
    if a <= KNEE { x } else { x.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AmpChannel, DriveKind};

    /// Nota de guitarra sintética: Lá da 5ª corda com harmônicos, ataque e decaimento.
    fn pluck(sr: f32, seconds: f32) -> Vec<f32> {
        (0..(sr * seconds) as usize)
            .map(|i| {
                let t = i as f32 / sr;
                let env = (-t * 2.5).exp();
                (1..8).map(|h| 0.25 / h as f32 * (2.0 * std::f32::consts::PI * 110.0 * h as f32 * t).sin()).sum::<f32>() * env
            })
            .collect()
    }

    /// Correlação normalizada máxima entre `a` atrasado de 0 a 120 amostras e `b`.
    fn best_correlation(a: &[f32], b: &[f32]) -> f32 {
        (0..120)
            .map(|lag| {
                let (x, y) = (&a[lag..], &b[..b.len() - lag]);
                let dot: f32 = x.iter().zip(y).map(|(p, q)| p * q).sum();
                let nx: f32 = x.iter().map(|v| v * v).sum::<f32>().sqrt();
                let ny: f32 = y.iter().map(|v| v * v).sum::<f32>().sqrt();
                dot / (nx * ny + 1e-12)
            })
            .fold(f32::MIN, f32::max)
    }

    /// Caso real de 31/10: com a sobreamostragem ligada o som ficava incompreensível no cabo.
    #[test]
    fn oversampling_keeps_the_same_sound() {
        let sr = 48000.0;
        for channel in AmpChannel::ALL {
            let mut s = Settings::default();
            s.amp.channel = channel;
            s.drive.on = channel != AmpChannel::Clean;
            s.reverb.on = false;
            s.gate.on = false;
            let render = |factor: usize| {
                let mut chain = SignalChain::new(sr, &s, factor);
                let mut buf = pluck(sr, 1.0);
                for block in buf.chunks_mut(144) {
                    chain.process(block);
                }
                buf
            };
            let (plain, over) = (render(1), render(4));
            let c = best_correlation(&over[4800..], &plain[4800..]);
            println!("{channel:?}: correlação {c:.4}");
            assert!(c > 0.9, "{channel:?}: correlação {c:.3} entre 1x e 4x");
        }
    }

    /// Custo da cadeia com tudo ligado, em fração de tempo real (`cargo test --release -- --ignored --nocapture custo`).
    #[test]
    #[ignore]
    fn custo_da_cadeia_inteira() {
        let sr = 48000.0;
        let mut s = Settings::default();
        s.comp.on = true;
        s.wah.on = true;
        s.octave.on = true;
        s.boost.on = true;
        s.drive.on = true;
        s.chorus.on = true;
        s.phaser.on = true;
        s.flanger.on = true;
        s.tremolo.on = true;
        s.amp.channel = AmpChannel::Lead;
        s.eq.on = true;
        s.delay.on = true;
        for factor in [1, 2, 4] {
            let mut chain = SignalChain::new(sr, &s, factor);
            let mut buf = vec![0.0f32; 144];
            let seconds = 20.0;
            let start = std::time::Instant::now();
            for b in 0..(seconds * sr / 144.0) as usize {
                for (i, v) in buf.iter_mut().enumerate() {
                    *v = 0.3 * (2.0 * std::f32::consts::PI * 110.0 * (b * 144 + i) as f32 / sr).sin();
                }
                chain.process(&mut buf);
            }
            let used = start.elapsed().as_secs_f32() / seconds;
            println!("{factor}x: {:.2}% de um núcleo", used * 100.0);
        }
    }

    #[test]
    fn everything_on_stays_finite_and_bounded() {
        let sr = 48000.0;
        for channel in AmpChannel::ALL {
            for kind in DriveKind::ALL {
                let mut s = Settings::default();
                s.master = 1.0;
                s.input_gain_db = 24.0;
                s.comp.on = true;
                s.drive.on = true;
                s.drive.kind = kind;
                s.drive.gain = 1.0;
                s.chorus.on = true;
                s.wah = crate::settings::WahCfg { on: true, sens: 1.0, resonance: 1.0, range: 1.0 };
                s.octave = crate::settings::OctaveCfg { on: true, sub: 1.0, up: 1.0, dry: 1.0 };
                s.boost = crate::settings::BoostCfg { on: true, level_db: 20.0, bright: 1.0 };
                s.phaser = crate::settings::PhaserCfg { on: true, rate: 1.0, depth: 1.0, feedback: 1.0 };
                s.flanger = crate::settings::FlangerCfg { on: true, rate: 1.0, depth: 1.0, feedback: 1.0 };
                s.tremolo = crate::settings::TremoloCfg { on: true, rate: 1.0, depth: 1.0, shape: 1.0 };
                s.gate.release_ms = 10.0;
                s.gate.hold_ms = 0.0;
                s.gate.range_db = 10.0;
                s.eq.low_cut_hz = 400.0;
                s.eq.high_cut_hz = 2000.0;
                s.amp.channel = channel;
                s.amp.gain = 1.0;
                s.amp.bass = 1.0;
                s.amp.treble = 1.0;
                s.amp.volume = 1.0;
                s.eq.on = true;
                s.eq.bands_db = [12.0; 5];
                s.delay.on = true;
                s.delay.feedback = 1.0;
                s.reverb.on = true;
                s.reverb.size = 1.0;
                s.reverb.mix = 1.0;
                let mut chain = SignalChain::new(sr, &s, 4);
                let mut x = 1u32;
                for _ in 0..200 {
                    let mut buf: Vec<f32> = (0..480)
                        .map(|_| {
                            x ^= x << 13;
                            x ^= x >> 17;
                            x ^= x << 5;
                            (x as f32 / u32::MAX as f32) - 0.5
                        })
                        .collect();
                    chain.process(&mut buf);
                    assert!(buf.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
                }
            }
        }
    }

    #[test]
    fn auto_gate_keeps_the_note_and_closes_in_silence() {
        let sr = 48000.0;
        let mut s = Settings::default();
        s.comp.on = false;
        s.amp.on = false;
        s.cab.on = false;
        s.reverb.on = false;
        let mut chain = SignalChain::new(sr, &s, 4);
        let mut seed = 7u32;
        let mut hiss = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            ((seed as f32 / u32::MAX as f32) - 0.5) * db_to_lin(-84.0) * 3.46
        };
        let hum = |i: usize| db_to_lin(-62.0) * 1.414 * (2.0 * std::f32::consts::PI * 60.0 * i as f32 / sr).sin();
        let mut n = 0usize;
        let mut run = |seconds: f32, note: &dyn Fn(f32) -> f32, check: &mut dyn FnMut(f32, bool)| {
            let blocks = (seconds * sr / 480.0) as usize;
            for _ in 0..blocks {
                let start_t = n as f32 / sr;
                let mut buf: Vec<f32> = (0..480).map(|k| hum(n + k) + hiss() + note((n + k) as f32 / sr)).collect();
                n += 480;
                chain.process(&mut buf);
                check(start_t, chain.gate_open());
            }
        };
        // ruído do cabo parado
        run(3.0, &|_| 0.0, &mut |_, _| {});
        let mut closed = false;
        run(0.5, &|_| 0.0, &mut |_, open| closed = !open);
        assert!(closed, "o gate tem que fechar no silêncio");
        // uma nota de Sol3 que morre devagar: de -12 dB até sumir em uns 6 s
        let t0 = 3.5f32; // 3 s + 0,5 s em blocos inteiros de 480
        let note = move |t: f32| db_to_lin(-12.0) * 1.414 * (-(t - t0) / 0.9).exp() * (2.0 * std::f32::consts::PI * 196.0 * t).sin();
        run(6.0, &note, &mut |t, open| {
            let level = -12.0 - 8.686 * (t - t0) / 0.9;
            if level > -60.0 {
                assert!(open, "cortou a nota aos {:.2} s, nível {level:.0} dB", t - t0);
            }
        });
        // silêncio de novo
        let mut closed = false;
        run(2.0, &|_| 0.0, &mut |_, open| closed = !open);
        assert!(closed, "o gate tem que fechar depois da nota");
    }

    #[test]
    fn mute_silences_the_output() {
        let mut s = Settings::default();
        s.mute = true;
        let mut chain = SignalChain::new(48000.0, &s, 2);
        let mut buf = vec![0.5f32; 4800];
        chain.process(&mut buf);
        assert!(buf[2400..].iter().all(|v| v.abs() < 1e-3));
    }
}
