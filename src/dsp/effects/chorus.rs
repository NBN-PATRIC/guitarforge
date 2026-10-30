use std::f32::consts::PI;

pub struct Chorus {
    buffer: Vec<f32>,
    write_pos: usize,
    phase: f32,
    rate_hz: f32,
    depth_s: f32,
    wet: f32,
    sample_rate: f32,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            buffer: vec![0.0; (sample_rate * 0.03) as usize],
            write_pos: 0,
            phase: 0.0,
            rate_hz: 1.5,
            depth_s: 0.003,
            wet: 0.25,
            sample_rate,
        }
    }

    pub fn set(&mut self, rate: f32, depth: f32, mix: f32) {
        self.rate_hz = 0.1 + rate * 4.9;
        self.depth_s = 0.0005 + depth * 0.0045;
        self.wet = mix * 0.5;
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let len = self.buffer.len();
        for s in buf.iter_mut() {
            self.buffer[self.write_pos] = *s;

            let lfo = (self.phase * 2.0 * PI).sin();
            let delay = (0.007 + self.depth_s * (1.0 + lfo) * 0.5) * self.sample_rate;
            let whole = delay as usize;
            let frac = delay - whole as f32;
            let i0 = (self.write_pos + len - whole) % len;
            let i1 = (i0 + len - 1) % len;
            let delayed = self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac;

            self.write_pos = (self.write_pos + 1) % len;
            self.phase += self.rate_hz / self.sample_rate;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            *s = *s * (1.0 - self.wet) + delayed * self.wet;
        }
    }
}
