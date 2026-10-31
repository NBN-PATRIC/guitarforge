use std::f32::consts::PI;

/// Filtro biquad (fórmulas do RBJ Audio EQ Cookbook), forma transposta direta II.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self::passthrough()
    }
}

struct Omega {
    cos: f32,
    sin: f32,
}

fn omega(sr: f32, freq: f32) -> Omega {
    let w0 = 2.0 * PI * freq.clamp(10.0, sr * 0.45) / sr;
    Omega { cos: w0.cos(), sin: w0.sin() }
}

impl Biquad {
    pub const fn passthrough() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }

    fn normalized(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, z1: 0.0, z2: 0.0 }
    }

    pub fn lowpass(sr: f32, freq: f32, q: f32) -> Self {
        let w = omega(sr, freq);
        let alpha = w.sin / (2.0 * q);
        let b = (1.0 - w.cos) / 2.0;
        Self::normalized(b, 1.0 - w.cos, b, 1.0 + alpha, -2.0 * w.cos, 1.0 - alpha)
    }

    pub fn highpass(sr: f32, freq: f32, q: f32) -> Self {
        let w = omega(sr, freq);
        let alpha = w.sin / (2.0 * q);
        let b = (1.0 + w.cos) / 2.0;
        Self::normalized(b, -(1.0 + w.cos), b, 1.0 + alpha, -2.0 * w.cos, 1.0 - alpha)
    }

    pub fn peaking(sr: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        let w = omega(sr, freq);
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = w.sin / (2.0 * q);
        Self::normalized(
            1.0 + alpha * a,
            -2.0 * w.cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * w.cos,
            1.0 - alpha / a,
        )
    }

    pub fn low_shelf(sr: f32, freq: f32, gain_db: f32) -> Self {
        let w = omega(sr, freq);
        let a = 10f32.powf(gain_db / 40.0);
        let k = 2.0 * a.sqrt() * w.sin / std::f32::consts::SQRT_2;
        Self::normalized(
            a * ((a + 1.0) - (a - 1.0) * w.cos + k),
            2.0 * a * ((a - 1.0) - (a + 1.0) * w.cos),
            a * ((a + 1.0) - (a - 1.0) * w.cos - k),
            (a + 1.0) + (a - 1.0) * w.cos + k,
            -2.0 * ((a - 1.0) + (a + 1.0) * w.cos),
            (a + 1.0) + (a - 1.0) * w.cos - k,
        )
    }

    pub fn high_shelf(sr: f32, freq: f32, gain_db: f32) -> Self {
        let w = omega(sr, freq);
        let a = 10f32.powf(gain_db / 40.0);
        let k = 2.0 * a.sqrt() * w.sin / std::f32::consts::SQRT_2;
        Self::normalized(
            a * ((a + 1.0) + (a - 1.0) * w.cos + k),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * w.cos),
            a * ((a + 1.0) + (a - 1.0) * w.cos - k),
            (a + 1.0) - (a - 1.0) * w.cos + k,
            2.0 * ((a - 1.0) - (a + 1.0) * w.cos),
            (a + 1.0) - (a - 1.0) * w.cos - k,
        )
    }

    /// Troca os coeficientes mantendo o estado interno (evita estalo ao girar um knob).
    pub fn retune(&mut self, other: Biquad) {
        self.b0 = other.b0;
        self.b1 = other.b1;
        self.b2 = other.b2;
        self.a1 = other.a1;
        self.a2 = other.a2;
    }

    /// Zera a memória do filtro (pedal religado não solta o resto do som antigo).
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.tick(*s);
        }
    }
}

#[cfg(test)]
pub(crate) fn steady_gain(mut f: Biquad, sr: f32, freq: f32) -> f32 {
    let n = (sr * 0.5) as usize;
    let mut peak = 0.0f32;
    for i in 0..n {
        let x = (2.0 * PI * freq * i as f32 / sr).sin();
        let y = f.tick(x);
        if i > n / 2 {
            peak = peak.max(y.abs());
        }
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn db(x: f32) -> f32 {
        20.0 * x.log10()
    }

    #[test]
    fn lowpass_and_highpass_pass_and_cut() {
        assert!((db(steady_gain(Biquad::lowpass(SR, 1000.0, 0.707), SR, 100.0))).abs() < 0.2);
        assert!(db(steady_gain(Biquad::lowpass(SR, 1000.0, 0.707), SR, 8000.0)) < -30.0);
        assert!((db(steady_gain(Biquad::highpass(SR, 100.0, 0.707), SR, 2000.0))).abs() < 0.2);
        assert!(db(steady_gain(Biquad::highpass(SR, 100.0, 0.707), SR, 20.0)) < -20.0);
    }

    #[test]
    fn peaking_and_shelves_hit_their_gain() {
        assert!((db(steady_gain(Biquad::peaking(SR, 1000.0, 1.0, 6.0), SR, 1000.0)) - 6.0).abs() < 0.2);
        assert!((db(steady_gain(Biquad::low_shelf(SR, 200.0, -9.0), SR, 30.0)) + 9.0).abs() < 0.5);
        assert!((db(steady_gain(Biquad::high_shelf(SR, 3000.0, 8.0), SR, 15000.0)) - 8.0).abs() < 0.6);
        assert!((db(steady_gain(Biquad::high_shelf(SR, 3000.0, 8.0), SR, 100.0))).abs() < 0.3);
    }
}
