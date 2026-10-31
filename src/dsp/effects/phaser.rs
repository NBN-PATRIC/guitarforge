use std::f32::consts::PI;

const STAGES: usize = 6;

/// Phaser: 6 passa-tudo de 1ª ordem com a frequência varrida pelo LFO, de 200 Hz até 3,2 kHz no máximo, somados ao
/// sinal direto (os buracos que andam no espectro). A realimentação acentua os buracos.
pub struct Phaser {
    sr: f32,
    phase: f32,
    rate_hz: f32,
    octaves: f32,
    feedback: f32,
    x1: [f32; STAGES],
    y1: [f32; STAGES],
    last: f32,
}

impl Phaser {
    pub fn new(sr: f32) -> Self {
        Self { sr, phase: 0.0, rate_hz: 0.5, octaves: 3.0, feedback: 0.4, x1: [0.0; STAGES], y1: [0.0; STAGES], last: 0.0 }
    }

    pub fn set(&mut self, rate: f32, depth: f32, feedback: f32) {
        self.rate_hz = 0.05 + rate * 3.95;
        self.octaves = 0.5 + depth * 3.5;
        self.feedback = feedback * 0.75;
    }

    pub fn clear(&mut self) {
        self.x1 = [0.0; STAGES];
        self.y1 = [0.0; STAGES];
        self.last = 0.0;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let lfo = 0.5 - 0.5 * (2.0 * PI * self.phase).cos();
            let freq = 200.0 * 2f32.powf(lfo * self.octaves);
            let t = (PI * freq / self.sr).tan();
            let a = (t - 1.0) / (t + 1.0);
            let mut x = *s + self.feedback * self.last;
            for i in 0..STAGES {
                let y = a * x + self.x1[i] - a * self.y1[i];
                self.x1[i] = x;
                self.y1[i] = y;
                x = y;
            }
            self.last = x;
            *s = 0.5 * (*s + x);
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
        let mut p = Phaser::new(48000.0);
        p.set(1.0, 1.0, 1.0);
        let mut buf: Vec<f32> = (0..96000).map(|i| if i % 97 == 0 { 0.8 } else { 0.0 }).collect();
        p.process(&mut buf);
        assert!(buf.iter().all(|v| v.is_finite() && v.abs() < 4.0));
    }

    #[test]
    fn notches_move_a_steady_tone_up_and_down() {
        let mut p = Phaser::new(48000.0);
        p.set(0.5, 1.0, 0.0);
        let mut buf: Vec<f32> = (0..96000).map(|i| (2.0 * PI * 800.0 * i as f32 / 48000.0).sin()).collect();
        p.process(&mut buf);
        let block_peaks: Vec<f32> = buf.chunks(960).map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs()))).collect();
        let hi = block_peaks.iter().cloned().fold(0.0f32, f32::max);
        let lo = block_peaks.iter().cloned().fold(f32::MAX, f32::min);
        assert!(hi > 0.9 && lo < 0.3, "{lo}..{hi}");
    }
}
