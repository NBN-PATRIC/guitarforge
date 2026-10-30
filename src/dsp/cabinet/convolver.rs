use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// Tamanho da partição: é também a latência que a convolução acrescenta (64 amostras = 1,3 ms a 48 kHz).
pub const PARTITION: usize = 64;
const FFT_LEN: usize = 2 * PARTITION;
const BINS: usize = PARTITION + 1;

/// Convolução particionada uniforme (overlap-save no domínio da frequência).
/// Tudo é alocado no `new`; `process` não aloca, pode rodar na thread de áudio.
pub struct Convolver {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    ir_spectra: Vec<Vec<Complex<f32>>>,
    history: Vec<Vec<Complex<f32>>>,
    history_pos: usize,
    window: Vec<f32>,
    fft_in: Vec<f32>,
    acc: Vec<Complex<f32>>,
    time_out: Vec<f32>,
    scratch_fwd: Vec<Complex<f32>>,
    scratch_inv: Vec<Complex<f32>>,
    output: Vec<f32>,
    fill: usize,
}

impl Convolver {
    pub fn new(ir: &[f32]) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(FFT_LEN);
        let inv = planner.plan_fft_inverse(FFT_LEN);
        let parts = ir.len().div_ceil(PARTITION).max(1);

        let mut frame = fwd.make_input_vec();
        let mut ir_spectra = Vec::with_capacity(parts);
        for p in 0..parts {
            frame.fill(0.0);
            let start = (p * PARTITION).min(ir.len());
            let end = (start + PARTITION).min(ir.len());
            frame[..end - start].copy_from_slice(&ir[start..end]);
            let mut spectrum = fwd.make_output_vec();
            fwd.process(&mut frame, &mut spectrum).expect("tamanhos da FFT conferidos");
            ir_spectra.push(spectrum);
        }

        Self {
            history: vec![vec![Complex::default(); BINS]; parts],
            history_pos: 0,
            window: vec![0.0; FFT_LEN],
            fft_in: fwd.make_input_vec(),
            acc: inv.make_input_vec(),
            time_out: inv.make_output_vec(),
            scratch_fwd: fwd.make_scratch_vec(),
            scratch_inv: inv.make_scratch_vec(),
            output: vec![0.0; PARTITION],
            fill: 0,
            ir_spectra,
            fwd,
            inv,
        }
    }

    fn run_block(&mut self) {
        let parts = self.ir_spectra.len();
        self.fft_in.copy_from_slice(&self.window);
        let _ = self.fwd.process_with_scratch(
            &mut self.fft_in,
            &mut self.history[self.history_pos],
            &mut self.scratch_fwd,
        );

        self.acc.fill(Complex::default());
        for (j, h) in self.ir_spectra.iter().enumerate() {
            let x = &self.history[(self.history_pos + parts - j) % parts];
            for ((a, xv), hv) in self.acc.iter_mut().zip(x).zip(h) {
                *a += xv * hv;
            }
        }
        self.acc[0].im = 0.0;
        self.acc[PARTITION].im = 0.0;
        let _ = self.inv.process_with_scratch(&mut self.acc, &mut self.time_out, &mut self.scratch_inv);

        let scale = 1.0 / FFT_LEN as f32;
        for (o, t) in self.output.iter_mut().zip(&self.time_out[PARTITION..]) {
            *o = t * scale;
        }
        self.history_pos = (self.history_pos + 1) % parts;
        self.window.copy_within(PARTITION.., 0);
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            self.window[PARTITION + self.fill] = *s;
            *s = self.output[self.fill];
            self.fill += 1;
            if self.fill == PARTITION {
                self.fill = 0;
                self.run_block();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pseudo_random(n: usize, seed: u32) -> Vec<f32> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32) - 0.5
            })
            .collect()
    }

    #[test]
    fn matches_direct_convolution_delayed_by_one_partition() {
        let ir = pseudo_random(300, 7);
        let input = pseudo_random(3000, 99);
        let mut out = input.clone();
        let mut conv = Convolver::new(&ir);
        for chunk in out.chunks_mut(37) {
            conv.process(chunk);
        }
        for n in PARTITION..input.len() {
            let k = n - PARTITION;
            let expected: f32 = (0..ir.len()).filter(|&j| j <= k).map(|j| ir[j] * input[k - j]).sum();
            assert!((out[n] - expected).abs() < 1e-4, "amostra {n}: {} vs {expected}", out[n]);
        }
    }
}
