use crate::dsp::biquad::Biquad;
use crate::dsp::time_coef;

/// Oitavador de uma nota por vez, à moda dos analógicos:
/// - a fundamental é achada depois de um passa-baixa, com histerese proporcional ao volume, e vira uma onda
///   quadrada na frequência da nota;
/// - oitava abaixo: um flip-flop troca de lado a cada ciclo completo (metade da frequência), vezes o volume, e um
///   passa-baixa arredonda a quadrada;
/// - oitava acima: o sinal vezes a quadrada da própria nota (retifica pelo ciclo da fundamental, não por amostra,
///   então o 2º harmônico forte do captador não devolve a fundamental).
///
/// Acorde confunde a detecção do ciclo (é assim nos pedais também); a dica do cartão avisa.
pub struct Octave {
    track: [Biquad; 2],
    sub_smooth: [Biquad; 2],
    up_band: Biquad,
    up_dc: Biquad,
    env: f32,
    env_coef: f32,
    above: bool,
    flip: f32,
    sub: f32,
    up: f32,
    dry: f32,
}

impl Octave {
    pub fn new(sr: f32) -> Self {
        let lp = Biquad::lowpass(sr, 500.0, 0.707);
        Self {
            track: [lp, lp],
            sub_smooth: [Biquad::lowpass(sr, 320.0, 0.707), Biquad::lowpass(sr, 320.0, 0.707)],
            up_band: Biquad::lowpass(sr, 2500.0, 0.707),
            up_dc: Biquad::highpass(sr, 80.0, 0.707),
            env: 0.0,
            env_coef: time_coef(20.0, sr),
            above: false,
            flip: 1.0,
            sub: 0.6,
            up: 0.0,
            dry: 0.7,
        }
    }

    pub fn set(&mut self, sub: f32, up: f32, dry: f32) {
        self.sub = sub;
        self.up = up;
        self.dry = dry;
    }

    pub fn clear(&mut self) {
        for b in self.track.iter_mut().chain(self.sub_smooth.iter_mut()) {
            b.reset();
        }
        self.up_band.reset();
        self.up_dc.reset();
        self.env = 0.0;
        self.above = false;
        self.flip = 1.0;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            let x = *s;
            let [t0, t1] = &mut self.track;
            let low = t1.tick(t0.tick(x));
            let a = low.abs();
            self.env = a.max(a + (self.env - a) * self.env_coef);
            let h = 0.15 * self.env;
            if !self.above && low > h {
                self.above = true;
            } else if self.above && low < -h {
                // terminou meio ciclo negativo depois de um positivo: um ciclo inteiro
                self.above = false;
                self.flip = -self.flip;
            }
            let [lp1, lp2] = &mut self.sub_smooth;
            let sub = lp2.tick(lp1.tick(self.env * self.flip)) * 1.3;
            let square = if self.above { 1.0 } else { -1.0 };
            let up = self.up_dc.tick(self.up_band.tick(x) * square) * 1.2;
            *s = self.dry * x + self.sub * sub + self.up * up;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use realfft::RealFftPlanner;
    use std::f32::consts::PI;

    fn strongest_hz(buf: &[f32], sr: f32) -> f32 {
        let n = buf.len();
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
        let mut input = buf.to_vec();
        let mut spec = fft.make_output_vec();
        fft.process(&mut input, &mut spec).unwrap();
        let (bin, _) = spec.iter().enumerate().skip(2).max_by(|a, b| a.1.norm().total_cmp(&b.1.norm())).unwrap();
        bin as f32 * sr / n as f32
    }

    fn run(sub: f32, up: f32, dry: f32) -> f32 {
        let mut o = Octave::new(48000.0);
        o.set(sub, up, dry);
        // Lá da 5ª corda com 3 harmônicos, como sai de um captador
        let mut buf: Vec<f32> = (0..48000)
            .map(|i| {
                let t = i as f32 / 48000.0;
                0.3 * (2.0 * PI * 110.0 * t).sin() + 0.1 * (2.0 * PI * 220.0 * t).sin() + 0.05 * (2.0 * PI * 330.0 * t).sin()
            })
            .collect();
        o.process(&mut buf);
        strongest_hz(&buf[16384..32768], 48000.0)
    }

    #[test]
    fn sub_alone_plays_one_octave_down() {
        let f = run(1.0, 0.0, 0.0);
        assert!((f - 55.0).abs() < 4.0, "{f} Hz");
    }

    #[test]
    fn up_alone_plays_one_octave_up() {
        let f = run(0.0, 1.0, 0.0);
        assert!((f - 220.0).abs() < 4.0, "{f} Hz");
    }
}
