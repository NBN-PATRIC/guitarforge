use crate::dsp::biquad::Biquad;
use crate::dsp::{db_to_lin, time_coef};

/// Boost limpo antes do drive e do amp: até +20 dB e brilho (prateleira de agudos de até +9 dB em 1,2 kHz,
/// como os treble boosters). Não satura sozinho: empurra o que vem depois.
pub struct Boost {
    sr: f32,
    shelf: Biquad,
    gain: f32,
    target: f32,
    coef: f32,
}

impl Boost {
    pub fn new(sr: f32) -> Self {
        Self { sr, shelf: Biquad::passthrough(), gain: 1.0, target: 1.0, coef: 1.0 - time_coef(20.0, sr) }
    }

    pub fn set(&mut self, level_db: f32, bright: f32) {
        self.target = db_to_lin(level_db);
        self.shelf.retune(Biquad::high_shelf(self.sr, 1200.0, bright * 9.0));
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        self.shelf.process(buf);
        for s in buf.iter_mut() {
            self.gain += (self.target - self.gain) * self.coef;
            *s *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn adds_the_asked_gain_to_a_low_note() {
        let mut b = Boost::new(48000.0);
        b.set(12.0, 0.0);
        let mut buf: Vec<f32> = (0..48000).map(|i| 0.1 * (2.0 * PI * 110.0 * i as f32 / 48000.0).sin()).collect();
        b.process(&mut buf);
        let peak = buf[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak / 0.1 - db_to_lin(12.0)).abs() < 0.1, "pico {peak}");
    }
}
