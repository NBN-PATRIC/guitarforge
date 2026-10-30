use std::f32::consts::PI;

/// Delay com repetições que escurecem (passa-baixa na realimentação) e tempo
/// que desliza ao mudar, sem estalo.
pub struct Delay {
    buffer: Vec<f32>,
    write_pos: usize,
    current: f32,
    target: f32,
    feedback: f32,
    wet: f32,
    damp_state: f32,
    damp_coef: f32,
    sample_rate: f32,
}

impl Delay {
    pub fn new(sample_rate: f32) -> Self {
        let target = 0.38 * sample_rate;
        Self {
            buffer: vec![0.0; (sample_rate * 2.0) as usize + 2],
            write_pos: 0,
            current: target,
            target,
            feedback: 0.35,
            wet: 0.25,
            damp_state: 0.0,
            damp_coef: 1.0 - (-2.0 * PI * 4500.0 / sample_rate).exp(),
            sample_rate,
        }
    }

    pub fn set(&mut self, time_ms: f32, feedback: f32, mix: f32) {
        let max = (self.buffer.len() - 2) as f32;
        self.target = (time_ms * 0.001 * self.sample_rate).clamp(1.0, max);
        self.feedback = feedback.clamp(0.0, 0.92);
        self.wet = mix * 0.8;
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.damp_state = 0.0;
        self.current = self.target;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let len = self.buffer.len();
        for s in buf.iter_mut() {
            self.current += (self.target - self.current) * 0.0002;
            let mut read = self.write_pos as f32 - self.current;
            if read < 0.0 {
                read += len as f32;
            }
            let i0 = read as usize % len;
            let i1 = (i0 + 1) % len;
            let frac = read - read.floor();
            let delayed = self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac;

            self.damp_state += self.damp_coef * (delayed - self.damp_state);
            self.buffer[self.write_pos] = *s + self.damp_state * self.feedback;
            self.write_pos = (self.write_pos + 1) % len;
            *s += delayed * self.wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_arrives_at_the_set_time() {
        let sr = 48000.0;
        let mut d = Delay::new(sr);
        d.set(250.0, 0.0, 1.0);
        d.clear();
        let mut buf = vec![0.0f32; 24000];
        buf[0] = 1.0;
        d.process(&mut buf);
        let (pos, _) = buf[1..].iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
        assert!(((pos + 1) as i64 - 12000).abs() <= 1, "eco em {}", pos + 1);
    }
}
