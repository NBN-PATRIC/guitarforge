use super::{db_to_lin, time_coef};

/// Gate com histerese (fecha 6 dB abaixo de onde abre) e tempo de espera, para cortar o zumbido dos single
/// coil entre as notas sem picotar o fim delas. Mede energia (RMS em ~10 ms), não pico: os estalinhos do jack
/// com mau contato chegam a -57 dBFS de pico sem energia nenhuma e seguravam o gate aberto no silêncio.
pub struct NoiseGate {
    open_thr: f32,
    close_thr: f32,
    mean_square: f32,
    ms_coef: f32,
    gain: f32,
    attack: f32,
    release: f32,
    hold: usize,
    hold_left: usize,
    /// Ganho com o gate fechado (0 = mudo).
    floor: f32,
    pub open: bool,
    sr: f32,
}

impl NoiseGate {
    pub fn new(sr: f32) -> Self {
        let mut g = Self {
            open_thr: 0.0,
            close_thr: 0.0,
            mean_square: 0.0,
            ms_coef: 1.0 - time_coef(10.0, sr),
            gain: 0.0,
            attack: time_coef(1.0, sr),
            release: time_coef(80.0, sr),
            hold: (0.05 * sr) as usize,
            hold_left: 0,
            floor: 0.0,
            open: false,
            sr,
        };
        g.set_threshold_db(-52.0);
        g
    }

    pub fn set_times(&mut self, hold_ms: f32, release_ms: f32, range_db: f32) {
        self.hold = (hold_ms.max(0.0) * 0.001 * self.sr) as usize;
        self.release = time_coef(release_ms.max(1.0), self.sr);
        self.floor = if range_db >= 90.0 { 0.0 } else { db_to_lin(-range_db) };
    }

    pub fn set_threshold_db(&mut self, db: f32) {
        self.open_thr = db_to_lin(db);
        self.close_thr = db_to_lin(db - 6.0);
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            self.mean_square += self.ms_coef * (*s * *s - self.mean_square);
            let env = self.mean_square.sqrt();
            if env > self.open_thr {
                self.open = true;
                self.hold_left = self.hold;
            } else if self.open {
                if env > self.close_thr {
                    self.hold_left = self.hold;
                } else if self.hold_left > 0 {
                    self.hold_left -= 1;
                } else {
                    self.open = false;
                }
            }
            let target = if self.open { 1.0 } else { self.floor };
            let coef = if target > self.gain { self.attack } else { self.release };
            self.gain = target + (self.gain - target) * coef;
            *s *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn tone(amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (2.0 * PI * 60.0 * i as f32 / 48000.0).sin()).collect()
    }

    #[test]
    fn hold_keeps_a_fading_note_until_the_time_runs_out() {
        let mut g = NoiseGate::new(48000.0);
        g.set_threshold_db(-52.0);
        g.set_times(500.0, 80.0, 90.0);
        let mut note = tone(db_to_lin(-20.0), 24000);
        g.process(&mut note);
        assert!(g.open);
        // a nota cai para baixo do limiar: 300 ms depois ainda aberto, 700 ms depois fechado
        let mut tail = tone(db_to_lin(-70.0), 14400);
        g.process(&mut tail);
        assert!(g.open, "fechou antes da espera");
        let mut more = tone(db_to_lin(-70.0), 19200);
        g.process(&mut more);
        assert!(!g.open, "não fechou depois da espera");
    }

    #[test]
    fn hum_stays_closed_and_playing_opens() {
        let mut g = NoiseGate::new(48000.0);
        let mut hum = tone(db_to_lin(-62.0), 48000);
        g.process(&mut hum);
        assert!(!g.open);
        assert!(hum[40000..].iter().all(|s| s.abs() < 1e-4));

        let mut note = tone(db_to_lin(-20.0), 4800);
        g.process(&mut note);
        assert!(g.open);
        assert!(note[2000..].iter().any(|s| s.abs() > 0.05));
    }

    #[test]
    fn clicks_from_a_bad_jack_do_not_hold_the_gate_open() {
        let mut g = NoiseGate::new(48000.0);
        let mut silence = vec![0.0f32; 48000];
        for i in (0..silence.len()).step_by(480) {
            silence[i] = db_to_lin(-45.0);
        }
        g.process(&mut silence);
        assert!(!g.open);
    }
}

const SEGMENT_S: f32 = 0.1;
const SEGMENTS: usize = 50;

/// Ruído de fundo para o gate automático: o menor nível (RMS em trechos de 100 ms) dos últimos 5 s.
/// Entre uma nota e outra o sinal sempre volta ao ruído, então o mínimo acompanha o ruído de verdade de quem
/// está tocando, com aquele cabo, naquele lugar; se o ruído mudar, em até 5 s o valor acompanha.
/// Sem alocação: pode rodar na thread de áudio.
pub struct FloorTracker {
    segment_len: usize,
    acc: f64,
    count: usize,
    ring: [f32; SEGMENTS],
    filled: usize,
    pos: usize,
    floor_db: Option<f32>,
}

impl FloorTracker {
    pub fn new(sr: f32) -> Self {
        Self {
            segment_len: ((sr * SEGMENT_S) as usize).max(1),
            acc: 0.0,
            count: 0,
            ring: [0.0; SEGMENTS],
            filled: 0,
            pos: 0,
            floor_db: None,
        }
    }

    pub fn push(&mut self, buf: &[f32]) {
        for &s in buf {
            self.acc += (s as f64) * (s as f64);
            self.count += 1;
            if self.count == self.segment_len {
                let db = 10.0 * (self.acc / self.count as f64).max(1e-14).log10();
                self.ring[self.pos] = db as f32;
                self.pos = (self.pos + 1) % SEGMENTS;
                self.filled = (self.filled + 1).min(SEGMENTS);
                self.floor_db = self.ring[..self.filled].iter().copied().reduce(f32::min);
                self.acc = 0.0;
                self.count = 0;
            }
        }
    }

    pub fn floor_db(&self) -> Option<f32> {
        self.floor_db
    }
}

/// Limiar do gate automático: 10 dB acima do ruído, nunca acima de -40 dB. Tocando sem pausa por mais de 5 s
/// o "ruído" medido sobe junto, e o limite impede o gate de comer o fim das notas.
pub fn auto_threshold_db(floor_db: f32) -> f32 {
    (floor_db + 10.0).clamp(-80.0, -40.0)
}

#[cfg(test)]
mod floor_tests {
    use super::*;
    use std::f32::consts::PI;

    fn tone(db: f32, seconds: f32, freq: f32) -> Vec<f32> {
        let amp = db_to_lin(db) * std::f32::consts::SQRT_2;
        (0..(48000.0 * seconds) as usize).map(|i| amp * (2.0 * PI * freq * i as f32 / 48000.0).sin()).collect()
    }

    #[test]
    fn follows_the_real_noise_and_survives_long_playing() {
        let mut f = FloorTracker::new(48000.0);
        f.push(&tone(-70.0, 3.0, 60.0));
        assert!((f.floor_db().unwrap() + 70.0).abs() < 0.5);
        assert!((auto_threshold_db(f.floor_db().unwrap()) + 60.0).abs() < 0.5);
        // tocando com pausas: as pausas mantêm o mínimo no ruído
        for _ in 0..4 {
            f.push(&tone(-20.0, 2.0, 196.0));
            f.push(&tone(-70.0, 0.3, 60.0));
        }
        assert!((f.floor_db().unwrap() + 70.0).abs() < 0.5);
        // 15 s sem pausa nenhuma: o limiar sobe mas para em -40
        f.push(&tone(-20.0, 15.0, 196.0));
        assert_eq!(auto_threshold_db(f.floor_db().unwrap()), -40.0);
        // mexeram no jack e o ruído subiu para -58: em 5 s o gate acompanha
        f.push(&tone(-58.0, 6.0, 60.0));
        assert!((f.floor_db().unwrap() + 58.0).abs() < 0.5);
    }
}
