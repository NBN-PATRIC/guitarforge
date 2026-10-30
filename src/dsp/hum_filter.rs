use std::f64::consts::PI;

/// Rede elétrica do Brasil (e Américas); a análise troca para 50 Hz se o cabo mostrar zumbido de 50.
pub const DEFAULT_MAINS_HZ: f32 = 60.0;
const HARMONICS: usize = 6;
/// Largura de cada entalhe: estreita o bastante para não encostar nas notas (a mais perto, Si2, fica 3,5 Hz
/// acima de 120 Hz) e larga o bastante para a rede variando uns décimos de Hz.
const BANDWIDTH_HZ: f64 = 2.5;

/// Entalhe em f64: em f32 o coeficiente de um entalhe tão estreito em 60 Hz erra o lugar por ~0,06 Hz e a
/// atenuação não passa de uns 23 dB.
struct Notch {
    b1: f64,
    a1: f64,
    a2: f64,
    gain: f64,
    z1: f64,
    z2: f64,
}

impl Notch {
    fn new(sr: f64, freq: f64) -> Self {
        let w0 = 2.0 * PI * freq / sr;
        let alpha = w0.sin() / (2.0 * (freq / BANDWIDTH_HZ));
        let a0 = 1.0 + alpha;
        Self { b1: -2.0 * w0.cos() / a0, a1: -2.0 * w0.cos() / a0, a2: (1.0 - alpha) / a0, gain: 1.0 / a0, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.gain * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.gain * x - self.a2 * y;
        y
    }
}

/// Tira o zumbido que os captadores single coil pegam da rede: entalhes na frequência da rede e nos 5
/// harmônicos seguintes (60, 120, ..., 360 Hz no Brasil).
pub struct HumFilter {
    sr: f64,
    mains: f32,
    notches: Vec<Notch>,
}

impl HumFilter {
    pub fn new(sr: f32, mains: f32) -> Self {
        let sr = sr as f64;
        let notches = (1..=HARMONICS).map(|k| Notch::new(sr, mains as f64 * k as f64)).collect();
        Self { sr, mains, notches }
    }

    /// Troca a frequência da rede sem alocar (pode ser chamada na thread de áudio).
    pub fn retune(&mut self, mains: f32) {
        if mains == self.mains {
            return;
        }
        self.mains = mains;
        for (k, n) in self.notches.iter_mut().enumerate() {
            *n = Notch::new(self.sr, mains as f64 * (k + 1) as f64);
        }
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let mut x = *s as f64;
            for n in &mut self.notches {
                x = n.tick(x);
            }
            *s = x as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_db(freq: f32) -> f32 {
        let sr = 48000.0;
        let mut f = HumFilter::new(sr, DEFAULT_MAINS_HZ);
        let n = (sr * 3.0) as usize;
        let mut buf: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin()).collect();
        f.process(&mut buf);
        let tail = &buf[n - (sr as usize)..];
        let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
        20.0 * (rms * std::f32::consts::SQRT_2).log10()
    }

    #[test]
    fn removes_hum_and_keeps_the_notes() {
        for hum in [60.0, 120.0, 180.0, 240.0] {
            assert!(gain_db(hum) < -30.0, "{hum} Hz: {}", gain_db(hum));
        }
        assert!(gain_db(60.3) < -12.0, "rede a 60,3 Hz: {}", gain_db(60.3));
        for note in [82.41, 110.0, 146.83, 196.0, 246.94, 329.63] {
            assert!(gain_db(note) > -0.5, "{note} Hz: {}", gain_db(note));
        }
        for close in [123.47, 185.0] {
            assert!(gain_db(close) > -2.0, "{close} Hz: {}", gain_db(close));
        }
    }
}
