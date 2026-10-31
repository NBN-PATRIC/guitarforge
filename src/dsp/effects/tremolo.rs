use std::f32::consts::PI;

/// Tremolo: o volume sobe e desce no ritmo do LFO. Formato de senoide (suave, de amp antigo) até quase quadrado
/// (picotado), sem quina: a onda é uma tanh cada vez mais íngreme, então não estala.
pub struct Tremolo {
    sr: f32,
    phase: f32,
    rate_hz: f32,
    depth: f32,
    steep: f32,
}

impl Tremolo {
    pub fn new(sr: f32) -> Self {
        Self { sr, phase: 0.0, rate_hz: 5.0, depth: 0.5, steep: 1.0 }
    }

    pub fn set(&mut self, rate: f32, depth: f32, shape: f32) {
        self.rate_hz = 1.0 + rate * 11.0;
        self.depth = depth;
        self.steep = 1.0 + shape * 8.0;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let norm = self.steep.tanh();
        for s in buf.iter_mut() {
            let lfo = (self.steep * (2.0 * PI * self.phase).sin()).tanh() / norm;
            *s *= 1.0 - self.depth * (0.5 - 0.5 * lfo);
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
    fn swings_between_full_and_reduced_volume() {
        let mut t = Tremolo::new(48000.0);
        t.set(0.3, 0.8, 0.0);
        let mut buf = vec![1.0f32; 48000];
        t.process(&mut buf);
        let max = buf.iter().cloned().fold(f32::MIN, f32::max);
        let min = buf.iter().cloned().fold(f32::MAX, f32::min);
        assert!(max > 0.99 && (min - 0.2).abs() < 0.01, "{min}..{max}");
    }

    #[test]
    fn square_shape_never_jumps() {
        let mut t = Tremolo::new(48000.0);
        t.set(1.0, 1.0, 1.0);
        let mut buf = vec![1.0f32; 48000];
        t.process(&mut buf);
        let worst = buf.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(worst < 0.05, "degrau de {worst}");
    }
}
