use crate::dsp::biquad::Biquad;
use crate::dsp::db_to_lin;
use crate::settings::{DriveCfg, DriveKind};

pub struct Drive {
    sr: f32,
    kind: DriveKind,
    hp: Biquad,
    lp: Biquad,
    /// O fuzz corta diferente em cima e embaixo: sem isto sobra um deslocamento que acompanha a nota (um "tum").
    dc: Biquad,
    gain: f32,
    level: f32,
}

impl Drive {
    pub fn new(sr: f32) -> Self {
        let mut d = Self {
            sr,
            kind: DriveKind::Overdrive,
            hp: Biquad::passthrough(),
            lp: Biquad::passthrough(),
            dc: Biquad::highpass(sr, 15.0, 0.707),
            gain: 1.0,
            level: 1.0,
        };
        d.set(&DriveCfg::default());
        d
    }

    pub fn set(&mut self, c: &DriveCfg) {
        let (hp_hz, gain_db, out) = match c.kind {
            DriveKind::Overdrive => (200.0, 6.0 + c.gain * 30.0, 0.6),
            DriveKind::Distortion => (100.0, 12.0 + c.gain * 36.0, 0.5),
            DriveKind::Fuzz => (50.0, 20.0 + c.gain * 36.0, 0.5),
        };
        self.kind = c.kind;
        self.hp.retune(Biquad::highpass(self.sr, hp_hz, 0.707));
        self.lp.retune(Biquad::lowpass(self.sr, 800.0 + c.tone * 6200.0, 0.707));
        self.gain = db_to_lin(gain_db);
        self.level = db_to_lin((c.level - 0.5) * 24.0) * out;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let x = self.hp.tick(*s) * self.gain;
            let y = match self.kind {
                DriveKind::Overdrive => x.tanh(),
                DriveKind::Distortion => cubic_clip(x),
                DriveKind::Fuzz => ((x + 0.25).tanh() - 0.25f32.tanh()).clamp(-0.8, 0.8),
            };
            *s = self.dc.tick(self.lp.tick(y)) * self.level;
        }
    }
}

fn cubic_clip(x: f32) -> f32 {
    if x.abs() >= 1.0 { x.signum() } else { 1.5 * (x - x * x * x / 3.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn fuzz_leaves_no_offset() {
        let sr = 48000.0;
        let mut d = Drive::new(sr);
        d.set(&DriveCfg { on: true, kind: DriveKind::Fuzz, gain: 1.0, tone: 0.5, level: 0.5 });
        let mut buf: Vec<f32> = (0..sr as usize).map(|i| 0.3 * (2.0 * PI * 110.0 * i as f32 / sr).sin()).collect();
        d.process(&mut buf);
        let tail = &buf[24000..];
        let mean = tail.iter().sum::<f32>() / tail.len() as f32;
        let peak = tail.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(mean.abs() < peak * 0.005, "deslocamento {mean} com pico {peak}");
    }
}
