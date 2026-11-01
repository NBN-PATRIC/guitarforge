use crate::dsp::room::RoomConvolver;

/// Reverb com dois motores, trocados por uma transição de 30 ms (nenhum corta o outro no meio do rabo):
/// - algoritmo no desenho do Freeverb (Jezar), em estéreo como o original: 8 combs e 4 all-pass por lado, os do lado
///   direito 23 amostras mais longos. Os dois lados soam a mesma sala mas sem bater um com o outro: no fone a sala
///   fica em volta, e não no meio da cabeça;
/// - sala por convolução (`RoomConvolver`, IR estéreo longo), com pré-atraso. A sala nova chega pronta da interface
///   e entra em transição de 50 ms com a anterior, que depois sai por `take_retired_room` para ser liberada fora do
///   áudio.
pub struct Reverb {
    left: Tank,
    right: Tank,
    mix: f32,
    use_room: bool,
    alg_gain: f32,
    room_gain: f32,
    gain_step: f32,
    /// O motor rodou desde a última limpeza (ao voltar do silêncio, limpa antes).
    alg_dirty: bool,
    room_dirty: bool,
    room: Option<Box<RoomConvolver>>,
    old_room: Option<Box<RoomConvolver>>,
    old_gain: f32,
    old_step: f32,
    retired: Option<Box<RoomConvolver>>,
    pre: Vec<f32>,
    pre_pos: usize,
    pre_delay: usize,
    mono: Vec<f32>,
    delayed: Vec<f32>,
    room_out: [Vec<f32>; 2],
    old_out: [Vec<f32>; 2],
}

struct Tank {
    combs: Vec<CombFilter>,
    allpasses: Vec<AllpassFilter>,
}

struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp: f32,
    lp_state: f32,
}

struct AllpassFilter {
    buffer: Vec<f32>,
    pos: usize,
}

const INPUT_GAIN: f32 = 0.015;
const WET_SCALE: f32 = 3.0;
const STEREO_SPREAD: usize = 23;
/// Ganho da sala por convolução (IR com energia 1 por lado) para soar no nível do algoritmo com o mesmo Mix
/// (teste `room_and_algorithm_have_the_same_level`).
const ROOM_WET: f32 = 0.95;
/// Pedaço processado de uma vez (a memória dos dois motores é alocada uma vez só).
const SUB: usize = 256;
pub const MAX_PRE_DELAY_MS: f32 = 150.0;

impl Tank {
    fn new(scale: f32, spread: usize) -> Self {
        let comb_lengths = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let allpass_lengths = [556, 441, 341, 225];
        let len = |n: usize| ((n + spread) as f32 * scale) as usize;
        Self {
            combs: comb_lengths.iter().map(|&n| CombFilter::new(len(n))).collect(),
            allpasses: allpass_lengths.iter().map(|&n| AllpassFilter::new(len(n))).collect(),
        }
    }

    fn clear(&mut self) {
        for c in &mut self.combs {
            c.buffer.fill(0.0);
            c.lp_state = 0.0;
        }
        for a in &mut self.allpasses {
            a.buffer.fill(0.0);
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let mut wet: f32 = self.combs.iter_mut().map(|c| c.process(input)).sum();
        for ap in &mut self.allpasses {
            wet = ap.process(wet);
        }
        wet
    }
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44100.0;
        let pre_len = (MAX_PRE_DELAY_MS * 0.001 * sample_rate) as usize + SUB + 1;
        Self {
            left: Tank::new(scale, 0),
            right: Tank::new(scale, STEREO_SPREAD),
            mix: 0.2,
            use_room: false,
            alg_gain: 1.0,
            room_gain: 0.0,
            gain_step: 1.0 / (0.030 * sample_rate),
            alg_dirty: false,
            room_dirty: false,
            room: None,
            old_room: None,
            old_gain: 0.0,
            old_step: 1.0 / (0.050 * sample_rate),
            retired: None,
            pre: vec![0.0; pre_len],
            pre_pos: 0,
            pre_delay: 0,
            mono: vec![0.0; SUB],
            delayed: vec![0.0; SUB],
            room_out: [vec![0.0; SUB], vec![0.0; SUB]],
            old_out: [vec![0.0; SUB], vec![0.0; SUB]],
        }
    }

    pub fn set(&mut self, size: f32, mix: f32) {
        let feedback = 0.70 + size.clamp(0.0, 1.0) * 0.28;
        for c in self.left.combs.iter_mut().chain(self.right.combs.iter_mut()) {
            c.feedback = feedback;
        }
        self.mix = mix;
    }

    /// Qual motor toca (`use_room` = sala por convolução) e o pré-atraso dela.
    pub fn set_room(&mut self, use_room: bool, pre_delay_ms: f32, sample_rate: f32) {
        self.use_room = use_room;
        let max = self.pre.len() - SUB - 1;
        self.pre_delay = ((pre_delay_ms.clamp(0.0, MAX_PRE_DELAY_MS) * 0.001 * sample_rate) as usize).min(max);
    }

    /// Sala nova (ou nenhuma). Se a de agora está soando, ela sai em transição; devolve a que pode ser liberada já.
    pub fn swap_room(&mut self, room: Option<Box<RoomConvolver>>) -> Option<Box<RoomConvolver>> {
        // sem sala agora (só a antiga saindo), ela continua saindo e a nova entra direto
        let audible = self.room_gain > 0.0;
        if audible && self.room.is_some() {
            let dispose = self.old_room.take();
            self.old_room = std::mem::replace(&mut self.room, room);
            self.old_gain = 1.0;
            dispose
        } else {
            std::mem::replace(&mut self.room, room)
        }
    }

    /// Sala que terminou de sair, para ser liberada fora da thread de áudio.
    pub fn take_retired_room(&mut self) -> Option<Box<RoomConvolver>> {
        self.retired.take()
    }

    pub fn clear(&mut self) {
        self.left.clear();
        self.right.clear();
        if let Some(r) = &mut self.room {
            r.reset();
        }
        self.pre.fill(0.0);
        self.alg_dirty = false;
        self.room_dirty = false;
        if self.retired.is_none() {
            self.retired = self.old_room.take();
        }
    }

    /// A sala ouve a soma dos dois lados e devolve um rabo diferente em cada um.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.chunks_mut(SUB).zip(right.chunks_mut(SUB)) {
            self.process_sub(l, r);
        }
    }

    fn process_sub(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let (alg_target, room_target) = if self.use_room { (0.0, 1.0) } else { (1.0, 0.0) };
        // motor que volta do silêncio começa limpo (o rabo de antes não reaparece)
        if alg_target > 0.0 && self.alg_gain == 0.0 && self.alg_dirty {
            self.left.clear();
            self.right.clear();
            self.alg_dirty = false;
        }
        if room_target > 0.0 && self.room_gain == 0.0 && self.room_dirty {
            if let Some(r) = &mut self.room {
                r.reset();
            }
            self.pre.fill(0.0);
            self.room_dirty = false;
        }
        let alg_on = self.alg_gain > 0.0 || alg_target > 0.0;
        // sem sala nova (voltou ao algoritmo), a antiga ainda sai em transição
        let room_on = (self.room_gain > 0.0 || room_target > 0.0) && (self.room.is_some() || self.old_room.is_some());

        for (m, (l, r)) in self.mono.iter_mut().zip(left.iter().zip(right.iter())) {
            *m = (*l + *r) * 0.5;
        }
        if room_on {
            let len = self.pre.len();
            for i in 0..n {
                self.pre[self.pre_pos] = self.mono[i];
                self.delayed[i] = self.pre[(self.pre_pos + len - self.pre_delay) % len];
                self.pre_pos = (self.pre_pos + 1) % len;
            }
            let [rl, rr] = &mut self.room_out;
            match &mut self.room {
                Some(room) => room.process(&self.delayed[..n], &mut rl[..n], &mut rr[..n]),
                None => {
                    rl[..n].fill(0.0);
                    rr[..n].fill(0.0);
                }
            }
            if let Some(old) = &mut self.old_room {
                let [ol, or] = &mut self.old_out;
                old.process(&self.delayed[..n], &mut ol[..n], &mut or[..n]);
            }
            self.room_dirty = true;
        }
        if alg_on {
            self.alg_dirty = true;
        }

        let dry_gain = 1.0 - self.mix * 0.3;
        let has_old = room_on && self.old_room.is_some();
        for i in 0..n {
            self.alg_gain = step_toward(self.alg_gain, alg_target, self.gain_step);
            self.room_gain = step_toward(self.room_gain, room_target, self.gain_step);
            let (mut wl, mut wr) = (0.0, 0.0);
            if alg_on {
                let input = self.mono[i] * INPUT_GAIN;
                let g = self.alg_gain * WET_SCALE;
                wl += g * self.left.process(input);
                wr += g * self.right.process(input);
            }
            if room_on {
                let g = self.room_gain * ROOM_WET;
                let (mut cl, mut cr) = (self.room_out[0][i], self.room_out[1][i]);
                if has_old {
                    self.old_gain = (self.old_gain - self.old_step).max(0.0);
                    let o = self.old_gain;
                    cl = cl * (1.0 - o) + self.old_out[0][i] * o;
                    cr = cr * (1.0 - o) + self.old_out[1][i] * o;
                }
                wl += g * cl;
                wr += g * cr;
            }
            left[i] = left[i] * dry_gain + wl * self.mix;
            right[i] = right[i] * dry_gain + wr * self.mix;
        }
        // a sala antiga terminou de sair (ou a sala calou): vai para a fila de liberar
        if self.old_room.is_some() && (self.old_gain == 0.0 || !room_on) && self.retired.is_none() {
            self.retired = self.old_room.take();
        }
    }
}

fn step_toward(x: f32, target: f32, step: f32) -> f32 {
    if x < target { (x + step).min(target) } else { (x - step).max(target) }
}

impl CombFilter {
    fn new(size: usize) -> Self {
        Self { buffer: vec![0.0; size], pos: 0, feedback: 0.84, damp: 0.25, lp_state: 0.0 }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.pos];
        self.lp_state = output * (1.0 - self.damp) + self.lp_state * self.damp;
        self.buffer[self.pos] = input + self.lp_state * self.feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }
}

impl AllpassFilter {
    fn new(size: usize) -> Self {
        Self { buffer: vec![0.0; size], pos: 0 }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.pos];
        self.buffer[self.pos] = input + buffered * 0.5;
        self.pos = (self.pos + 1) % self.buffer.len();
        buffered - input
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse() -> (Vec<f32>, Vec<f32>) {
        let mut r = Reverb::new(48000.0);
        r.set(1.0, 1.0);
        let (mut l, mut rt) = (vec![0.0f32; 48000 * 6], vec![0.0f32; 48000 * 6]);
        l[0] = 1.0;
        rt[0] = 1.0;
        r.process(&mut l, &mut rt);
        (l, rt)
    }

    #[test]
    fn tail_decays() {
        let (l, r) = impulse();
        for buf in [&l, &r] {
            let early: f32 = buf[4800..9600].iter().map(|x| x * x).sum();
            let late: f32 = buf[48000 * 5..].iter().map(|x| x * x).sum();
            assert!(buf.iter().all(|x| x.is_finite()));
            assert!(early > 0.0 && late < early * 0.05);
        }
    }

    #[test]
    fn the_two_sides_are_different_rooms() {
        let (l, r) = impulse();
        let (a, b) = (&l[4800..48000], &r[4800..48000]);
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm = (a.iter().map(|x| x * x).sum::<f32>() * b.iter().map(|x| x * x).sum::<f32>()).sqrt();
        assert!((dot / norm).abs() < 0.5, "lados parecidos demais: correlação {}", dot / norm);
    }

    use crate::dsp::room;
    use crate::settings::ReverbKind;

    const SR: f32 = 48000.0;

    /// Notas de guitarra sintéticas (Lá e Mi com harmônicos), uma a cada meio segundo.
    fn notes(seconds: f32) -> Vec<f32> {
        (0..(SR * seconds) as usize)
            .map(|i| {
                let t = (i as f32 / SR) % 0.5;
                let f = if (i as f32 / SR) % 1.0 < 0.5 { 110.0 } else { 164.8 };
                (1..8).map(|h| 0.2 / h as f32 * (2.0 * std::f32::consts::PI * f * h as f32 * t).sin()).sum::<f32>()
                    * (-t * 4.0).exp()
            })
            .collect()
    }

    /// Só o rabo (saída menos o seco) de `input` passando pelo reverb em blocos de 144 (3 ms).
    fn wet_of(rev: &mut Reverb, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (input.to_vec(), input.to_vec());
        for (a, b) in l.chunks_mut(144).zip(r.chunks_mut(144)) {
            rev.process(a, b);
        }
        let dry = 1.0 - rev.mix * 0.3;
        for (x, y) in l.iter_mut().chain(r.iter_mut()).zip(input.iter().chain(input)) {
            *x -= y * dry;
        }
        (l, r)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn room_reverb(kind: ReverbKind) -> Reverb {
        let mut rev = Reverb::new(SR);
        rev.set(0.5, 0.3);
        rev.set_room(true, 0.0, SR);
        assert!(rev.swap_room(room::build(kind, None, SR as u32)).is_none());
        rev
    }

    /// Trocar o algoritmo pela Sala com o mesmo Mix não pode pular de volume (calibra `ROOM_WET`).
    #[test]
    fn room_and_algorithm_have_the_same_level() {
        let input = notes(4.0);
        let mut alg = Reverb::new(SR);
        alg.set(0.5, 0.3);
        let (al, _) = wet_of(&mut alg, &input);
        let reference = rms(&al[48000..]);
        let mut worst = 0.0f32;
        for kind in ReverbKind::ROOMS {
            let (rl, rr) = wet_of(&mut room_reverb(kind), &input);
            let db = 20.0 * (0.5 * (rms(&rl[48000..]) + rms(&rr[48000..])) / reference).log10();
            println!("{kind:?}: {db:+.1} dB em relação ao algoritmo");
            worst = worst.max(db.abs());
        }
        assert!(worst < 4.0, "sala {worst:.1} dB longe do algoritmo");
    }

    /// Maior salto entre duas amostras seguidas.
    fn max_jump(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    /// Algoritmo para sala e de volta, no meio das notas: transição de 30 ms, sem estalo e sem NaN.
    #[test]
    fn switching_engines_does_not_click() {
        let input = notes(3.0);
        let mut rev = Reverb::new(SR);
        rev.set(0.5, 0.5);
        assert!(rev.swap_room(room::build(ReverbKind::Hall, None, SR as u32)).is_none());
        let (mut l, mut r) = (input.clone(), input.clone());
        let mut at = 0;
        for (a, b) in l.chunks_mut(144).zip(r.chunks_mut(144)) {
            if at == 48000 {
                rev.set_room(true, 20.0, SR);
            }
            if at == 96000 {
                rev.set_room(false, 20.0, SR);
            }
            rev.process(a, b);
            at += 144;
        }
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        let normal = max_jump(&l[24000..47000]).max(max_jump(&input[24000..47000]));
        for at in [48000, 96000] {
            let jump = max_jump(&l[at - 200..at + 3000]).max(max_jump(&r[at - 200..at + 3000]));
            assert!(jump < normal * 1.5, "troca em {at}: salto {jump} contra {normal} normal");
        }
    }

    /// Custo da sala (média e pior bloco de 3 ms), com a de 8 s como pior caso de arquivo:
    /// `cargo test --release -- --ignored --nocapture custo_da_sala`.
    #[test]
    #[ignore]
    fn custo_da_sala() {
        let input = notes(20.0);
        let long = room::RoomShape { rt60: 7.5, pre_ms: 0.0, early: 0, early_ms: 1.0, bright_hz: 8000.0, dark_hz: 2000.0 };
        let cases: Vec<(&str, Reverb)> = vec![
            ("algoritmo", {
                let mut r = Reverb::new(SR);
                r.set(0.5, 0.3);
                r
            }),
            ("quarto", room_reverb(ReverbKind::SmallRoom)),
            ("sala", room_reverb(ReverbKind::Room)),
            ("igreja", room_reverb(ReverbKind::Church)),
            ("arquivo de 8 s", {
                let mut r = Reverb::new(SR);
                r.set(0.5, 0.3);
                r.set_room(true, 0.0, SR);
                let (a, b) = room::generate(&long, SR as u32);
                r.swap_room(Some(Box::new(room::RoomConvolver::new(&a, &b))));
                r
            }),
        ];
        for (name, mut rev) in cases {
            let (mut l, mut r) = (input.clone(), input.clone());
            let mut worst = 0.0f64;
            let start = std::time::Instant::now();
            for (a, b) in l.chunks_mut(144).zip(r.chunks_mut(144)) {
                let t = std::time::Instant::now();
                rev.process(a, b);
                worst = worst.max(t.elapsed().as_secs_f64());
            }
            let used = start.elapsed().as_secs_f64() / 20.0;
            println!("{name}: {:.2}% de um núcleo, pior bloco de 3 ms: {:.3} ms", used * 100.0, worst * 1000.0);
        }
    }

    /// Sala nova no meio do som: a antiga sai em 50 ms e depois volta para ser liberada fora do áudio.
    #[test]
    fn new_room_crossfades_and_the_old_one_is_retired() {
        let input = notes(2.0);
        let mut rev = room_reverb(ReverbKind::Room);
        let (mut l, mut r) = (input.clone(), input.clone());
        let (first, rest) = l.split_at_mut(48000);
        let (first_r, rest_r) = r.split_at_mut(48000);
        rev.process(first, first_r);
        assert!(rev.swap_room(room::build(ReverbKind::Church, None, SR as u32)).is_none(), "a de agora sai em transição");
        let mut retired_at = None;
        for (i, (a, b)) in rest.chunks_mut(144).zip(rest_r.chunks_mut(144)).enumerate() {
            rev.process(a, b);
            if retired_at.is_none() && rev.take_retired_room().is_some() {
                retired_at = Some(i * 144);
            }
        }
        let at = retired_at.expect("a sala antiga não voltou para ser liberada");
        assert!((2400 - 144..=2400 + 144).contains(&at), "saiu em {at} amostras, esperado ~2400 (50 ms)");
        let normal = max_jump(&l[24000..47000]);
        let jump = max_jump(&l[47800..52000]);
        assert!(jump < normal * 1.5, "troca de sala: salto {jump} contra {normal} normal");
    }
}
