use std::f32::consts::PI;

use crate::dsp::biquad::Biquad;

/// Atraso da cópia: 15 ms, balançando 1,5 ms para cada lado bem devagar (a dobra não fica parada como um eco).
const BASE_MS: f32 = 15.0;
const SWING_MS: f32 = 1.5;
const RATE_HZ: f32 = 0.23;
/// Abaixo disso a cópia não entra: o grave fica no meio, como numa mixagem.
const MONO_BELOW_HZ: f32 = 130.0;

/// Estéreo por dobra (a ideia do Spread do TONE3000): uma cópia atrasada do som entra somada num lado e subtraída no
/// outro. Somando os dois lados volta exatamente o som de antes, então em caixa mono nada muda (sem o filtro pente de
/// um atraso comum), e no fone o som sai de dentro da cabeça.
pub struct Spread {
    sr: f32,
    buffer: Vec<f32>,
    write: usize,
    phase: f32,
    hp: [Biquad; 2],
    width: f32,
}

impl Spread {
    pub fn new(sr: f32) -> Self {
        let hp = Biquad::highpass(sr, MONO_BELOW_HZ, 0.707);
        Self { sr, buffer: vec![0.0; (sr * 0.03) as usize], write: 0, phase: 0.0, hp: [hp, hp], width: 0.5 }
    }

    /// `width`: 0 = mono, 1 = o mais aberto.
    pub fn set(&mut self, width: f32) {
        self.width = width.clamp(0.0, 1.0);
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.hp.iter_mut().for_each(Biquad::reset);
    }

    /// `left` traz o som (mono, igual ao `right`); sai o estéreo nos dois.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let len = self.buffer.len();
        let amount = 0.7 * self.width;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let x = *l;
            self.buffer[self.write] = x;
            let swing = (2.0 * PI * self.phase).sin();
            let delay = (BASE_MS + SWING_MS * swing) * 0.001 * self.sr;
            let whole = delay as usize;
            let frac = delay - whole as f32;
            let i0 = (self.write + len - whole) % len;
            let i1 = (i0 + len - 1) % len;
            let copy = self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac;
            let [h1, h2] = &mut self.hp;
            let side = h2.tick(h1.tick(copy)) * amount;
            *l = x + side;
            *r = x - side;
            self.write = (self.write + 1) % len;
            self.phase += RATE_HZ / self.sr;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(freq: f32, width: f32) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let sr = 48000.0;
        let mut s = Spread::new(sr);
        s.set(width);
        let input: Vec<f32> = (0..48000).map(|i| 0.5 * (2.0 * PI * freq * i as f32 / sr).sin()).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        s.process(&mut l, &mut r);
        (input, l, r)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn mono_sum_is_the_original_sound() {
        let (input, l, r) = run(440.0, 1.0);
        let worst = input.iter().zip(l.iter().zip(&r)).map(|(x, (a, b))| (0.5 * (a + b) - x).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "mono difere em {worst}");
    }

    #[test]
    fn bass_stays_in_the_middle_and_the_rest_opens() {
        // 60 Hz: os dois lados quase iguais; 1 kHz: bem diferentes
        let (_, l, r) = run(60.0, 1.0);
        let diff: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
        assert!(rms(&diff[24000..]) < 0.1 * rms(&l[24000..]), "grave abriu: {}", rms(&diff[24000..]));
        let (_, l, r) = run(1000.0, 1.0);
        let diff: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
        assert!(rms(&diff[24000..]) > 0.5 * rms(&l[24000..]), "médio não abriu: {}", rms(&diff[24000..]));
        // largura zero: mono
        let (_, l, r) = run(1000.0, 0.0);
        assert_eq!(l, r);
    }
}
