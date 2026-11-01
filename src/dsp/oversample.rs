//! Sobreamostragem para os trechos que saturam (drive e amp). A saturação cria harmônicos acima de 24 kHz que, a
//! 48 kHz, voltam espelhados para dentro do audível como um chiado metálico que não segue a nota (aliasing).
//! Rodando a 2x ou 4x, esses harmônicos caem acima da metade da taxa alta e o filtro da descida os tira.
//!
//! Cada degrau de 2x é um FIR meia-banda de fase linear (seno cardinal com janela de Kaiser, 47 coeficientes,
//! ~70 dB de rejeição, plano até ~19 kHz), em forma polifásica. Atraso: 0,48 ms por degrau a 48 kHz.

use std::f32::consts::PI;

const TAPS: usize = 47;
const KAISER_BETA: f32 = 6.8;
/// Bloco máximo processado de uma vez; o resto vai em pedaços (nada aloca na thread de áudio).
const MAX_BLOCK: usize = 2048;

fn bessel_i0(x: f32) -> f32 {
    let (mut sum, mut term) = (1.0f32, 1.0f32);
    for k in 1..32 {
        term *= (x / (2.0 * k as f32)).powi(2);
        sum += term;
    }
    sum
}

/// Meia-banda: corte em 1/4 da taxa alta, coeficientes pares (fora o centro) nulos, ganho 1 em DC.
fn halfband() -> [f32; TAPS] {
    let c = (TAPS - 1) as f32 / 2.0;
    let mut h = [0.0f32; TAPS];
    for (n, v) in h.iter_mut().enumerate() {
        let t = n as f32 - c;
        let sinc = if t == 0.0 { 1.0 } else { (0.5 * PI * t).sin() / (0.5 * PI * t) };
        let r = t / c;
        let window = bessel_i0(KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(KAISER_BETA);
        *v = 0.5 * sinc * window;
    }
    let sum: f32 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= sum);
    h
}

/// Histórico com cópia dupla: a janela mais recente é sempre uma fatia contínua (sem módulo no laço).
struct History {
    buf: Vec<f32>,
    pos: usize,
    n: usize,
}

impl History {
    fn new(n: usize) -> Self {
        Self { buf: vec![0.0; 2 * n], pos: 0, n }
    }

    #[inline]
    fn push(&mut self, x: f32) {
        self.pos = if self.pos == 0 { self.n - 1 } else { self.pos - 1 };
        self.buf[self.pos] = x;
        self.buf[self.pos + self.n] = x;
    }

    /// `taps[k]` multiplica a k-ésima amostra mais recente.
    #[inline]
    fn dot(&self, taps: &[f32]) -> f32 {
        self.buf[self.pos..self.pos + taps.len()].iter().zip(taps).map(|(a, b)| a * b).sum()
    }
}

/// Sobe 2x: as duas fases do filtro sobre a mesma entrada (o zero intercalado nunca é multiplicado).
struct Up2 {
    even: Vec<f32>,
    odd: Vec<f32>,
    hist: History,
}

/// Desce 2x: filtra e calcula só as amostras que ficam.
struct Down2 {
    taps: Vec<f32>,
    hist: History,
}

impl Up2 {
    fn new(h: &[f32]) -> Self {
        let even: Vec<f32> = h.iter().step_by(2).map(|v| 2.0 * v).collect();
        let odd: Vec<f32> = h.iter().skip(1).step_by(2).map(|v| 2.0 * v).collect();
        Self { hist: History::new(even.len()), even, odd }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> [f32; 2] {
        self.hist.push(x);
        [self.hist.dot(&self.even), self.hist.dot(&self.odd)]
    }
}

impl Down2 {
    fn new(h: &[f32]) -> Self {
        Self { taps: h.to_vec(), hist: History::new(h.len()) }
    }

    #[inline]
    fn tick(&mut self, a: f32, b: f32) -> f32 {
        self.hist.push(a);
        let y = self.hist.dot(&self.taps);
        self.hist.push(b);
        y
    }
}

pub struct Oversampler {
    factor: usize,
    up: Vec<Up2>,
    down: Vec<Down2>,
    a: Vec<f32>,
    b: Vec<f32>,
}

impl Oversampler {
    /// `factor`: 1, 2 ou 4.
    pub fn new(factor: usize) -> Self {
        let factor: usize = match factor {
            0 | 1 => 1,
            2 | 3 => 2,
            _ => 4,
        };
        let stages = factor.trailing_zeros() as usize;
        let h = halfband();
        Self {
            factor,
            up: (0..stages).map(|_| Up2::new(&h)).collect(),
            down: (0..stages).map(|_| Down2::new(&h)).collect(),
            a: Vec::with_capacity(MAX_BLOCK * factor),
            b: Vec::with_capacity(MAX_BLOCK * factor),
        }
    }

    /// Atraso somado, em amostras da taxa normal.
    #[cfg(test)]
    pub fn latency(&self) -> f32 {
        let per_filter = (TAPS - 1) as f32 / 2.0;
        (0..self.up.len()).map(|i| 2.0 * per_filter / (2 << i) as f32).sum()
    }

    /// Sobe a taxa, roda `f` na taxa alta e volta.
    pub fn process(&mut self, buf: &mut [f32], mut f: impl FnMut(&mut [f32])) {
        if self.factor == 1 {
            f(buf);
            return;
        }
        for chunk in buf.chunks_mut(MAX_BLOCK) {
            self.a.clear();
            self.a.extend_from_slice(chunk);
            for stage in &mut self.up {
                self.b.clear();
                for &x in &self.a {
                    self.b.extend_from_slice(&stage.tick(x));
                }
                std::mem::swap(&mut self.a, &mut self.b);
            }
            f(&mut self.a);
            for stage in self.down.iter_mut().rev() {
                self.b.clear();
                for pair in self.a.as_chunks::<2>().0 {
                    self.b.push(stage.tick(pair[0], pair[1]));
                }
                std::mem::swap(&mut self.a, &mut self.b);
            }
            chunk.copy_from_slice(&self.a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use realfft::RealFftPlanner;

    const SR: f32 = 48000.0;

    fn magnitude_db(buf: &[f32], hz: f32) -> f32 {
        let n = buf.len();
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
        // janela de Hann para a energia de um tom não vazar para os vizinhos
        let mut input: Vec<f32> =
            buf.iter().enumerate().map(|(i, v)| v * (0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())).collect();
        let mut spec = fft.make_output_vec();
        fft.process(&mut input, &mut spec).unwrap();
        let bin = (hz * n as f32 / SR).round() as usize;
        let peak = (bin - 2..=bin + 2).map(|b| spec[b].norm()).fold(0.0f32, f32::max);
        20.0 * (peak / n as f32 + 1e-12).log10()
    }

    fn clipped_tone(factor: usize) -> Vec<f32> {
        let mut os = Oversampler::new(factor);
        let mut buf: Vec<f32> = (0..32768).map(|i| 0.9 * (2.0 * PI * 5000.0 * i as f32 / SR).sin()).collect();
        os.process(&mut buf, |hi| hi.iter_mut().for_each(|v| *v = (*v * 8.0).tanh()));
        buf.split_off(4096)
    }

    #[test]
    fn plain_signal_passes_unchanged_apart_from_the_delay() {
        for factor in [2, 4] {
            let mut os = Oversampler::new(factor);
            let tone = |t: f32| (2.0 * PI * 1000.0 * t / SR).sin();
            let mut buf: Vec<f32> = (0..8192).map(|i| tone(i as f32)).collect();
            os.process(&mut buf, |_| {});
            // a 4x o atraso é fracionário (34,5 amostras): compara com o tom atrasado exatamente
            let delay = os.latency();
            let worst = buf[200..].iter().enumerate().map(|(i, v)| (v - tone((i + 200) as f32 - delay)).abs()).fold(0.0f32, f32::max);
            assert!(worst < 0.01, "{factor}x: diferença {worst} com atraso de {delay}");
        }
    }

    #[test]
    fn clipping_at_4x_folds_far_less_back_into_the_audible_band() {
        // 7º harmônico de 5 kHz = 35 kHz, que a 48 kHz volta como 13 kHz
        let plain = magnitude_db(&clipped_tone(1), 13000.0);
        let over = magnitude_db(&clipped_tone(4), 13000.0);
        assert!(over < plain - 30.0, "sem: {plain:.1} dB, com: {over:.1} dB");
        // e o que é música (o 3º harmônico, 15 kHz) continua lá
        let h3_plain = magnitude_db(&clipped_tone(1), 15000.0);
        let h3_over = magnitude_db(&clipped_tone(4), 15000.0);
        assert!((h3_plain - h3_over).abs() < 2.0, "3º harmônico: {h3_plain:.1} contra {h3_over:.1}");
    }
}
