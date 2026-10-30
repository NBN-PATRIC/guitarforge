pub mod amp;
pub mod biquad;
pub mod cabinet;
pub mod effects;
pub mod hum_filter;
pub mod hum_meter;
pub mod noise_gate;
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

/// Cadeia fixa: entrada > filtro de 60 Hz > gate > comp > drive > chorus > amp > gabinete > EQ > delay > reverb > volume.
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
    drive_on: bool,
    drive: effects::Drive,
    chorus_on: bool,
    chorus: effects::Chorus,
    amp_on: bool,
    amp: amp::AmpSim,
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
    pub fn new(sr: f32, settings: &Settings) -> Self {
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
            drive_on: false,
            drive: effects::Drive::new(sr),
            chorus_on: false,
            chorus: effects::Chorus::new(sr),
            amp_on: false,
            amp: amp::AmpSim::new(sr),
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
        if !self.auto_gate {
            self.gate.set_threshold_db(s.gate.threshold_db);
        }
        self.comp_on = s.comp.on;
        self.comp.set(s.comp.sustain, s.comp.level);
        self.drive_on = s.drive.on;
        self.drive.set(&s.drive);

        if s.chorus.on && !self.chorus_on {
            self.chorus.clear();
        }
        self.chorus_on = s.chorus.on;
        self.chorus.set(s.chorus.rate, s.chorus.depth, s.chorus.mix);

        self.amp_on = s.amp.on;
        self.amp.set(&s.amp);
        self.cab_on = s.cab.on;
        self.cabinet.set(&s.cab);
        self.eq_on = s.eq.on;
        self.eq.set(&s.eq.bands_db);

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
        if self.drive_on {
            self.drive.process(buf);
        }
        if self.chorus_on {
            self.chorus.process(buf);
        }
        if self.amp_on {
            self.amp.process(buf);
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
                let mut chain = SignalChain::new(sr, &s);
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
        let mut chain = SignalChain::new(sr, &s);
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
        let mut chain = SignalChain::new(48000.0, &s);
        let mut buf = vec![0.5f32; 4800];
        chain.process(&mut buf);
        assert!(buf[2400..].iter().all(|v| v.abs() < 1e-3));
    }
}
