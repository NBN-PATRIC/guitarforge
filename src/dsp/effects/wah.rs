use std::f32::consts::PI;

use crate::dsp::time_coef;

/// Auto-wah: a força da palhetada abre o filtro (o "uá" sem pedal de expressão). Filtro de estado variável
/// (topologia TPT, estável com a frequência mudando a cada amostra) em passa-faixa, de 350 Hz até o topo da faixa.
pub struct Wah {
    sr: f32,
    env: f32,
    attack: f32,
    release: f32,
    /// Nível (dB) a partir do qual o filtro começa a abrir; 30 dB acima ele está todo aberto.
    open_db: f32,
    top_hz: f32,
    k: f32,
    makeup: f32,
    ic1: f32,
    ic2: f32,
}

const BOTTOM_HZ: f32 = 350.0;

impl Wah {
    pub fn new(sr: f32) -> Self {
        let mut w = Self {
            sr,
            env: 0.0,
            attack: time_coef(4.0, sr),
            release: time_coef(150.0, sr),
            open_db: -40.0,
            top_hz: 2200.0,
            k: 0.25,
            makeup: 1.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        w.set(0.5, 0.5, 0.6);
        w
    }

    /// `sens`: quanto a palhetada abre; `resonance`: quão estreito e cantado; `range`: até onde abre (1 a 3,5 kHz).
    pub fn set(&mut self, sens: f32, resonance: f32, range: f32) {
        self.open_db = -20.0 - sens * 40.0;
        let q = 1.5 + resonance * 8.5;
        self.k = 1.0 / q;
        // o passa-faixa normalizado tem pico 1; quanto mais estreito, menos energia passa
        self.makeup = 0.8 + 0.12 * q;
        self.top_hz = 1000.0 + range * 2500.0;
    }

    pub fn clear(&mut self) {
        self.env = 0.0;
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let span = (self.top_hz / BOTTOM_HZ).ln();
        for s in buf.iter_mut() {
            let x = *s;
            let a = x.abs();
            let coef = if a > self.env { self.attack } else { self.release };
            self.env = a + (self.env - a) * coef;
            let pos = ((20.0 * (self.env + 1e-9).log10() - self.open_db) / 30.0).clamp(0.0, 1.0);
            let freq = BOTTOM_HZ * (span * pos).exp();
            let g = (PI * freq / self.sr).tan();
            let a1 = 1.0 / (1.0 + g * (g + self.k));
            let a2 = g * a1;
            let v3 = x - self.ic2;
            let v1 = a1 * self.ic1 + a2 * v3;
            let v2 = self.ic2 + a2 * self.ic1 + g * a2 * v3;
            self.ic1 = 2.0 * v1 - self.ic1;
            self.ic2 = 2.0 * v2 - self.ic2;
            *s = self.k * v1 * self.makeup;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level_at(freq: f32, amp: f32) -> f32 {
        let mut w = Wah::new(48000.0);
        w.set(0.7, 0.5, 0.6);
        let mut buf: Vec<f32> = (0..24000).map(|i| amp * (2.0 * PI * freq * i as f32 / 48000.0).sin()).collect();
        w.process(&mut buf);
        buf[12000..].iter().fold(0.0f32, |m, v| m.max(v.abs())) / amp
    }

    #[test]
    fn soft_playing_keeps_the_filter_low_and_hard_playing_opens_it() {
        // tocando fraco o filtro fica embaixo: 400 Hz passa bem mais que 1,8 kHz; forte, inverte
        assert!(level_at(400.0, 0.003) > 2.0 * level_at(1800.0, 0.003));
        assert!(level_at(1800.0, 0.5) > 2.0 * level_at(400.0, 0.5));
    }
}
