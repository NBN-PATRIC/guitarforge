use crate::dsp::{db_to_lin, lin_to_db, time_coef};

pub struct Compressor {
    attack: f32,
    release: f32,
    env: f32,
    threshold_db: f32,
    slope: f32,
    makeup: f32,
}

impl Compressor {
    pub fn new(sr: f32) -> Self {
        let mut c = Self {
            attack: time_coef(5.0, sr),
            release: time_coef(120.0, sr),
            env: 0.0,
            threshold_db: -27.0,
            slope: 0.75,
            makeup: 1.0,
        };
        c.set(0.5, 0.5);
        c
    }

    pub fn set(&mut self, sustain: f32, level: f32) {
        const RATIO: f32 = 4.0;
        self.threshold_db = -12.0 - sustain * 30.0;
        self.slope = 1.0 - 1.0 / RATIO;
        let auto_makeup = -self.threshold_db * self.slope * 0.6;
        self.makeup = db_to_lin(auto_makeup + (level - 0.5) * 24.0);
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let a = s.abs();
            let coef = if a > self.env { self.attack } else { self.release };
            self.env = a + (self.env - a) * coef;
            let over = lin_to_db(self.env) - self.threshold_db;
            let gain_db = if over > 0.0 { -over * self.slope } else { 0.0 };
            *s *= db_to_lin(gain_db) * self.makeup;
        }
    }
}
