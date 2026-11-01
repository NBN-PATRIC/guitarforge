//! Sala por convolução: IR longo (até 8 s), estéreo, com o mesmo atraso do IR de gabinete (64 amostras).
//!
//! O IR é dividido em três camadas, cada uma com partições do seu tamanho:
//! - cabeça: blocos de 64, do início até 960 amostras, calculada inteira a cada 64 amostras;
//! - meio: blocos de 512, de 960 a 8128;
//! - cauda: blocos de 4096, de 8128 até o fim.
//!
//! Uma camada de bloco B cobre o IR a partir de 2B - 64. Assim, o bloco de entrada dela fica pronto, a conta é
//! espalhada pelos B/64 blocos de 64 seguintes (nenhum bloco de áudio paga a conta inteira) e o resultado sai no
//! período depois desse, já alinhado com a cabeça. Juntas num bloco só ficam apenas as FFTs de fim de período
//! (8192 pontos na cauda, uma vez a cada 85 ms). Tudo é alocado no `new`; `process` não aloca.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use crate::dsp::cabinet::ImpulseResponse;
use crate::settings::ReverbKind;

/// Bloco da cabeça: é a latência da sala.
pub const LATENCY: usize = 64;
const BLOCKS: [usize; 3] = [LATENCY, 512, 4096];
/// IR de sala mais longo aceito (o resto do arquivo é cortado).
pub const MAX_SECONDS: f32 = 8.0;

/// Uma camada: convolução particionada uniforme com bloco `block`.
struct Layer {
    block: usize,
    /// Blocos de 64 por período da camada (1 na cabeça).
    subs: usize,
    /// Bloco de 64 atual dentro do período.
    k: usize,
    parts: usize,
    bins: usize,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    /// Espectros das partições do IR, esquerdo e direito, `parts * bins` cada.
    h: [Vec<Complex<f32>>; 2],
    /// Espectros das últimas entradas (linha de atraso no domínio da frequência), `parts * bins`.
    fdl: Vec<Complex<f32>>,
    pos: usize,
    window: Vec<f32>,
    fft_in: Vec<f32>,
    acc: [Vec<Complex<f32>>; 2],
    time_out: Vec<f32>,
    scratch_fwd: Vec<Complex<f32>>,
    scratch_inv: Vec<Complex<f32>>,
    out: [Vec<f32>; 2],
}

impl Layer {
    fn new(planner: &mut RealFftPlanner<f32>, block: usize, left: &[f32], right: &[f32]) -> Option<Self> {
        if left.is_empty() {
            return None;
        }
        let fwd = planner.plan_fft_forward(2 * block);
        let inv = planner.plan_fft_inverse(2 * block);
        let bins = block + 1;
        let parts = left.len().div_ceil(block);
        let mut frame = fwd.make_input_vec();
        let mut spectrum = fwd.make_output_vec();
        let mut h = [Vec::with_capacity(parts * bins), Vec::with_capacity(parts * bins)];
        for (side, ir) in [left, right].into_iter().enumerate() {
            for p in 0..parts {
                frame.fill(0.0);
                let start = p * block;
                let end = (start + block).min(ir.len());
                frame[..end - start].copy_from_slice(&ir[start..end]);
                fwd.process(&mut frame, &mut spectrum).expect("tamanhos da FFT conferidos");
                h[side].extend_from_slice(&spectrum);
            }
        }
        Some(Self {
            block,
            subs: block / LATENCY,
            k: 0,
            parts,
            bins,
            h,
            fdl: vec![Complex::default(); parts * bins],
            pos: 0,
            window: vec![0.0; 2 * block],
            fft_in: fwd.make_input_vec(),
            acc: [inv.make_input_vec(), inv.make_input_vec()],
            time_out: inv.make_output_vec(),
            scratch_fwd: fwd.make_scratch_vec(),
            scratch_inv: inv.make_scratch_vec(),
            out: [vec![0.0; block], vec![0.0; block]],
            fwd,
            inv,
        })
    }

    fn reset(&mut self) {
        self.k = 0;
        self.pos = 0;
        self.fdl.fill(Complex::default());
        self.window.fill(0.0);
        for side in 0..2 {
            self.acc[side].fill(Complex::default());
            self.out[side].fill(0.0);
        }
    }

    /// Saída da camada para a amostra `fill` do bloco de 64 atual.
    #[inline]
    fn sample(&self, fill: usize) -> (f32, f32) {
        let i = self.k * LATENCY + fill;
        (self.out[0][i], self.out[1][i])
    }

    /// Fim de um bloco de 64: `input` são as 64 amostras dele.
    fn step(&mut self, input: &[f32]) {
        let at = self.block + self.k * LATENCY;
        self.window[at..at + LATENCY].copy_from_slice(input);
        if self.subs == 1 {
            self.transform_input();
            self.accumulate(0, self.parts);
            self.finish();
        } else {
            let k = self.k;
            self.accumulate(k * self.parts / self.subs, (k + 1) * self.parts / self.subs);
            if k == self.subs - 1 {
                self.finish();
                self.transform_input();
            }
        }
        self.k = (self.k + 1) % self.subs;
    }

    fn transform_input(&mut self) {
        self.pos = (self.pos + 1) % self.parts;
        self.fft_in.copy_from_slice(&self.window);
        let slot = &mut self.fdl[self.pos * self.bins..(self.pos + 1) * self.bins];
        let _ = self.fwd.process_with_scratch(&mut self.fft_in, slot, &mut self.scratch_fwd);
        self.window.copy_within(self.block.., 0);
    }

    /// Soma as partições `from..to`, cada uma com a entrada de `p` períodos atrás.
    fn accumulate(&mut self, from: usize, to: usize) {
        let bins = self.bins;
        for p in from..to {
            let slot = (self.pos + self.parts - p) % self.parts;
            let x = &self.fdl[slot * bins..(slot + 1) * bins];
            for side in 0..2 {
                let h = &self.h[side][p * bins..(p + 1) * bins];
                for ((a, xv), hv) in self.acc[side].iter_mut().zip(x).zip(h) {
                    *a += xv * hv;
                }
            }
        }
    }

    fn finish(&mut self) {
        let scale = 1.0 / (2 * self.block) as f32;
        for side in 0..2 {
            let acc = &mut self.acc[side];
            acc[0].im = 0.0;
            acc[self.block].im = 0.0;
            let _ = self.inv.process_with_scratch(acc, &mut self.time_out, &mut self.scratch_inv);
            acc.fill(Complex::default());
            for (o, t) in self.out[side].iter_mut().zip(&self.time_out[self.block..]) {
                *o = t * scale;
            }
        }
    }
}

/// Convolução de uma entrada mono com um IR estéreo (devolve só o som da sala).
pub struct RoomConvolver {
    layers: Vec<Layer>,
    block: [f32; LATENCY],
    fill: usize,
}

impl RoomConvolver {
    /// `left` e `right` já na taxa do motor e com o nível certo (ver `prepare`).
    pub fn new(left: &[f32], right: &[f32]) -> Self {
        assert_eq!(left.len(), right.len());
        let mut planner = RealFftPlanner::<f32>::new();
        let mut layers = Vec::new();
        for (i, &b) in BLOCKS.iter().enumerate() {
            let start = if i == 0 { 0 } else { 2 * b - LATENCY };
            let end = BLOCKS.get(i + 1).map_or(left.len(), |&next| 2 * next - LATENCY);
            let (start, end) = (start.min(left.len()), end.min(left.len()));
            if let Some(layer) = Layer::new(&mut planner, b, &left[start..end], &right[start..end]) {
                layers.push(layer);
            }
        }
        Self { layers, block: [0.0; LATENCY], fill: 0 }
    }

    /// Esquece o som de antes (memória zerada; os espectros do IR ficam).
    pub fn reset(&mut self) {
        for l in &mut self.layers {
            l.reset();
        }
        self.block = [0.0; LATENCY];
        self.fill = 0;
    }

    pub fn process(&mut self, input: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        for ((&x, l), r) in input.iter().zip(out_l.iter_mut()).zip(out_r.iter_mut()) {
            self.block[self.fill] = x;
            let (mut sl, mut sr) = (0.0, 0.0);
            for layer in &self.layers {
                let (a, b) = layer.sample(self.fill);
                sl += a;
                sr += b;
            }
            *l = sl;
            *r = sr;
            self.fill += 1;
            if self.fill == LATENCY {
                self.fill = 0;
                for layer in &mut self.layers {
                    layer.step(&self.block);
                }
            }
        }
    }
}

/// IR estéreo pronto para a sala: cortado em `MAX_SECONDS`, com fade de 10 ms no fim e energia média 1 por lado
/// (o "Mix" decide o volume; salas diferentes ficam no mesmo nível).
pub fn prepare(mut left: Vec<f32>, mut right: Vec<f32>, rate: u32) -> (Vec<f32>, Vec<f32>) {
    let max = (MAX_SECONDS * rate as f32) as usize;
    let len = left.len().max(right.len()).min(max);
    left.resize(len, 0.0);
    right.resize(len, 0.0);
    let fade = len.min(rate as usize / 100);
    for (i, k) in (len - fade..len).enumerate() {
        let g = 1.0 - i as f32 / fade as f32;
        left[k] *= g;
        right[k] *= g;
    }
    let energy: f32 = left.iter().chain(&right).map(|x| x * x).sum::<f32>() / 2.0;
    if energy > 1e-12 {
        let g = 1.0 / energy.sqrt();
        left.iter_mut().chain(right.iter_mut()).for_each(|x| *x *= g);
    }
    (left, right)
}

/// Como uma sala de fábrica soa: tempo de reverberação, primeiras reflexões e quanto o agudo morre antes.
#[derive(Clone, Copy, Debug)]
pub struct RoomShape {
    /// Tempo para o som cair 60 dB.
    pub rt60: f32,
    /// Atraso até a primeira reflexão.
    pub pre_ms: f32,
    /// Quantas reflexões separadas antes do rabo, e em quanto tempo.
    pub early: usize,
    pub early_ms: f32,
    /// Corte do agudo no começo do rabo e no fim (o ar e as paredes comem o agudo mais rápido).
    pub bright_hz: f32,
    pub dark_hz: f32,
}

pub fn shape(kind: ReverbKind) -> Option<RoomShape> {
    let s = |rt60, pre_ms, early, early_ms, bright_hz, dark_hz| RoomShape { rt60, pre_ms, early, early_ms, bright_hz, dark_hz };
    Some(match kind {
        ReverbKind::Algorithm | ReverbKind::File => return None,
        ReverbKind::SmallRoom => s(0.4, 2.0, 8, 12.0, 9000.0, 3000.0),
        ReverbKind::Studio => s(0.7, 5.0, 10, 20.0, 10000.0, 3500.0),
        ReverbKind::Room => s(1.2, 9.0, 12, 30.0, 9000.0, 2500.0),
        ReverbKind::Hall => s(2.2, 18.0, 14, 45.0, 8000.0, 2000.0),
        ReverbKind::Church => s(3.8, 30.0, 16, 70.0, 6500.0, 1500.0),
    })
}

/// Gera o IR de uma sala de fábrica (nada vem de arquivo): reflexões espalhadas e um rabo de ruído que cai
/// exponencialmente e escurece com o tempo. Cada lado tem o próprio sorteio, então a sala fica em volta e não no meio.
pub fn generate(shape: &RoomShape, rate: u32) -> (Vec<f32>, Vec<f32>) {
    let sr = rate as f32;
    let len = (((shape.pre_ms * 0.001 + shape.rt60 * 1.1) * sr) as usize).min((MAX_SECONDS * sr) as usize);
    let side = |seed: u32| {
        let mut x = seed;
        let mut rand = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        let mut ir = vec![0.0f32; len];
        let onset = (shape.pre_ms * 0.001 * sr) as usize;
        let build = (shape.early_ms * 0.001 * sr).max(1.0);
        let decay = -6.9078 / (shape.rt60 * sr);
        // rabo: ruído com envelope que sobe durante as reflexões e depois cai 60 dB em rt60
        let mut lp = 0.0f32;
        for (n, v) in ir.iter_mut().enumerate().skip(onset) {
            let t = (n - onset) as f32;
            let fc = shape.bright_hz * (shape.dark_hz / shape.bright_hz).powf((t / (shape.rt60 * sr)).min(1.0));
            let a = (-2.0 * std::f32::consts::PI * fc / sr).exp();
            lp = (1.0 - a) * rand() + a * lp;
            *v = lp * (decay * t).exp() * (t / build).min(1.0);
        }
        // primeiras reflexões: pulsos curtos de sinal sorteado, cada vez mais fracos
        let peak = ir.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
        for i in 0..shape.early {
            let at = onset + ((rand() * 0.5 + 0.5) * build) as usize;
            let amp = peak * 1.5 * (1.0 - i as f32 / shape.early as f32) * rand().signum();
            for (j, w) in [0.25, 0.5, 0.25].into_iter().enumerate() {
                if let Some(v) = ir.get_mut(at + j) {
                    *v += amp * w;
                }
            }
        }
        // sem grave embolado: passa-altas de 1 polo em 120 Hz
        let a = (-2.0 * std::f32::consts::PI * 120.0 / sr).exp();
        let (mut px, mut py) = (0.0f32, 0.0f32);
        for v in ir.iter_mut() {
            let y = a * (py + *v - px);
            px = *v;
            py = y;
            *v = y;
        }
        ir
    };
    let seed = (shape.rt60 * 1000.0) as u32;
    prepare(side(0x9E37_79B9 ^ seed), side(0x85EB_CA6B ^ seed.rotate_left(7)), rate)
}

/// A sala que os ajustes pedem: uma de fábrica, a do arquivo (`file`) ou nenhuma (reverb por algoritmo ou arquivo
/// que falta). Monta tudo aqui, fora da thread de áudio.
pub fn build(kind: ReverbKind, file: Option<&ImpulseResponse>, rate: u32) -> Option<Box<RoomConvolver>> {
    let (l, r) = match (kind, file) {
        (ReverbKind::File, Some(ir)) => {
            let (l, r) = ir.stereo(rate);
            prepare(l, r, rate)
        }
        _ => generate(&shape(kind)?, rate),
    };
    Some(Box::new(RoomConvolver::new(&l, &r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
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

    /// As três camadas juntas dão a convolução direta, atrasada de 64 amostras, nos dois lados, em blocos de
    /// tamanhos quaisquer (como chegam do driver).
    #[test]
    fn three_layers_match_direct_convolution() {
        let (hl, hr) = (noise(11000, 3), noise(11000, 5));
        let input = noise(30000, 9);
        let mut conv = RoomConvolver::new(&hl, &hr);
        assert_eq!(conv.layers.len(), 3);
        let (mut ol, mut or) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        let mut at = 0;
        for size in [37, 300, 1, 1024, 144, 999].iter().cycle() {
            if at >= input.len() {
                break;
            }
            let end = (at + size).min(input.len());
            conv.process(&input[at..end], &mut ol[at..end], &mut or[at..end]);
            at = end;
        }
        for n in (LATENCY..input.len()).step_by(97) {
            let k = n - LATENCY;
            let direct = |h: &[f32]| -> f32 { (0..h.len().min(k + 1)).map(|j| h[j] * input[k - j]).sum() };
            let (el, er) = (direct(&hl), direct(&hr));
            assert!((ol[n] - el).abs() < 2e-3, "esquerdo, amostra {n}: {} vs {el}", ol[n]);
            assert!((or[n] - er).abs() < 2e-3, "direito, amostra {n}: {} vs {er}", or[n]);
        }
    }

    #[test]
    fn reset_forgets_the_old_sound() {
        let (hl, hr) = (noise(9000, 1), noise(9000, 2));
        let mut conv = RoomConvolver::new(&hl, &hr);
        let input = noise(20000, 4);
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        conv.process(&input, &mut l, &mut r);
        conv.reset();
        let zeros = vec![0.0; 20000];
        conv.process(&zeros, &mut l, &mut r);
        assert!(l.iter().chain(&r).all(|v| *v == 0.0));
    }

    /// Energia em janelas de 50 ms, em dB.
    fn envelope_db(ir: &[f32], sr: usize) -> Vec<f32> {
        ir.chunks(sr / 20).map(|c| 10.0 * (c.iter().map(|x| x * x).sum::<f32>() + 1e-20).log10()).collect()
    }

    #[test]
    fn factory_rooms_decay_at_their_rt60_and_are_wide() {
        for kind in ReverbKind::ROOMS {
            let s = shape(kind).unwrap();
            let (l, r) = generate(&s, 48000);
            assert!(l.iter().chain(&r).all(|v| v.is_finite()));
            let energy: f32 = l.iter().chain(&r).map(|x| x * x).sum::<f32>() / 2.0;
            assert!((energy - 1.0).abs() < 1e-3, "{kind:?}: energia {energy}");
            // queda medida entre 10% e 60% do rt60, extrapolada para 60 dB
            let env = envelope_db(&l, 48000);
            let at = |frac: f32| env[((s.pre_ms * 0.001 + s.rt60 * frac) * 20.0) as usize];
            let rt = 60.0 * s.rt60 * 0.5 / (at(0.1) - at(0.6));
            assert!((rt / s.rt60 - 1.0).abs() < 0.35, "{kind:?}: rt60 medido {rt:.2} s, pedido {}", s.rt60);
            // os dois lados são salas diferentes (som em volta, não no meio da cabeça)
            let dot: f32 = l.iter().zip(&r).map(|(a, b)| a * b).sum();
            assert!(dot.abs() < 0.3, "{kind:?}: correlação {dot}");
        }
    }
}
