/// Reverb no desenho do Freeverb (Jezar): 8 combs em paralelo e 4 all-pass em série.
pub struct Reverb {
    combs: Vec<CombFilter>,
    allpasses: Vec<AllpassFilter>,
    mix: f32,
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

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44100.0;
        let comb_lengths = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let allpass_lengths = [556, 441, 341, 225];
        Self {
            combs: comb_lengths.iter().map(|&n| CombFilter::new((n as f32 * scale) as usize)).collect(),
            allpasses: allpass_lengths.iter().map(|&n| AllpassFilter::new((n as f32 * scale) as usize)).collect(),
            mix: 0.2,
        }
    }

    pub fn set(&mut self, size: f32, mix: f32) {
        let feedback = 0.70 + size.clamp(0.0, 1.0) * 0.28;
        for c in &mut self.combs {
            c.feedback = feedback;
        }
        self.mix = mix;
    }

    pub fn clear(&mut self) {
        for c in &mut self.combs {
            c.buffer.fill(0.0);
            c.lp_state = 0.0;
        }
        for a in &mut self.allpasses {
            a.buffer.fill(0.0);
        }
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let dry_gain = 1.0 - self.mix * 0.3;
        for s in buf.iter_mut() {
            let input = *s * INPUT_GAIN;
            let mut wet: f32 = self.combs.iter_mut().map(|c| c.process(input)).sum();
            for ap in &mut self.allpasses {
                wet = ap.process(wet);
            }
            *s = *s * dry_gain + wet * WET_SCALE * self.mix;
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

    #[test]
    fn tail_decays() {
        let mut r = Reverb::new(48000.0);
        r.set(1.0, 1.0);
        let mut buf = vec![0.0f32; 48000 * 6];
        buf[0] = 1.0;
        r.process(&mut buf);
        let early: f32 = buf[4800..9600].iter().map(|x| x * x).sum();
        let late: f32 = buf[48000 * 5..].iter().map(|x| x * x).sum();
        assert!(buf.iter().all(|x| x.is_finite()));
        assert!(early > 0.0 && late < early * 0.05);
    }
}
