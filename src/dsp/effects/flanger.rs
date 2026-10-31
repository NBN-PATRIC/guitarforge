use std::f32::consts::PI;

/// Flanger: atraso curto (0,3 a 4,3 ms) varrido pelo LFO e somado ao direto, com realimentação; o "avião" do
/// jato que sobe e desce. O chorus é o primo de atraso mais longo e sem realimentação.
pub struct Flanger {
    sr: f32,
    buffer: Vec<f32>,
    write: usize,
    phase: f32,
    rate_hz: f32,
    depth_s: f32,
    feedback: f32,
}

impl Flanger {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            buffer: vec![0.0; (sr * 0.01) as usize],
            write: 0,
            phase: 0.0,
            rate_hz: 0.3,
            depth_s: 0.002,
            feedback: 0.5,
        }
    }

    pub fn set(&mut self, rate: f32, depth: f32, feedback: f32) {
        self.rate_hz = 0.05 + rate * 1.95;
        self.depth_s = 0.0002 + depth * 0.0038;
        self.feedback = feedback * 0.85;
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let len = self.buffer.len();
        for s in buf.iter_mut() {
            let lfo = 0.5 - 0.5 * (2.0 * PI * self.phase).cos();
            let delay = (0.0003 + self.depth_s * lfo) * self.sr;
            let whole = delay as usize;
            let frac = delay - whole as f32;
            let i0 = (self.write + len - whole - 1) % len;
            let i1 = (i0 + len - 1) % len;
            let delayed = self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac;
            self.buffer[self.write] = *s + self.feedback * delayed;
            self.write = (self.write + 1) % len;
            *s = 0.6 * (*s + delayed);
            self.phase += self.rate_hz / self.sr;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_bounded_with_maximum_feedback() {
        let mut f = Flanger::new(48000.0);
        f.set(1.0, 1.0, 1.0);
        let mut buf: Vec<f32> = (0..96000).map(|i| (2.0 * PI * 440.0 * i as f32 / 48000.0).sin() * 0.5).collect();
        f.process(&mut buf);
        assert!(buf.iter().all(|v| v.is_finite() && v.abs() < 5.0));
    }
}
