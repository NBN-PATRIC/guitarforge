/// Reverb no desenho do Freeverb (Jezar), em estéreo como o original: 8 combs e 4 all-pass por lado, os do lado
/// direito 23 amostras mais longos. Os dois lados soam a mesma sala mas sem bater um com o outro: no fone a sala fica
/// em volta, e não no meio da cabeça.
pub struct Reverb {
    left: Tank,
    right: Tank,
    mix: f32,
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
        Self { left: Tank::new(scale, 0), right: Tank::new(scale, STEREO_SPREAD), mix: 0.2 }
    }

    pub fn set(&mut self, size: f32, mix: f32) {
        let feedback = 0.70 + size.clamp(0.0, 1.0) * 0.28;
        for c in self.left.combs.iter_mut().chain(self.right.combs.iter_mut()) {
            c.feedback = feedback;
        }
        self.mix = mix;
    }

    pub fn clear(&mut self) {
        self.left.clear();
        self.right.clear();
    }

    /// A sala ouve a soma dos dois lados e devolve um rabo diferente em cada um.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let dry_gain = 1.0 - self.mix * 0.3;
        let wet_gain = WET_SCALE * self.mix;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = (*l + *r) * 0.5 * INPUT_GAIN;
            let (wl, wr) = (self.left.process(input), self.right.process(input));
            *l = *l * dry_gain + wl * wet_gain;
            *r = *r * dry_gain + wr * wet_gain;
        }
    }
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
}
