use crate::dsp::biquad::Biquad;
use crate::settings::EQ_FREQS;

pub struct ParametricEq {
    sr: f32,
    bands: [Biquad; 5],
}

impl ParametricEq {
    pub fn new(sr: f32) -> Self {
        Self { sr, bands: [Biquad::passthrough(); 5] }
    }

    pub fn set(&mut self, gains_db: &[f32; 5]) {
        for ((band, &freq), &gain) in self.bands.iter_mut().zip(&EQ_FREQS).zip(gains_db) {
            band.retune(Biquad::peaking(self.sr, freq, 1.1, gain));
        }
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for band in &mut self.bands {
            band.process(buf);
        }
    }
}
