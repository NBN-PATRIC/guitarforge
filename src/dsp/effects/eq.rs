use crate::dsp::biquad::Biquad;
use crate::settings::EQ_FREQS;

pub struct ParametricEq {
    sr: f32,
    bands: [Biquad; 5],
    low_cut: Option<[Biquad; 2]>,
    high_cut: Option<[Biquad; 2]>,
}

impl ParametricEq {
    pub fn new(sr: f32) -> Self {
        Self { sr, bands: [Biquad::passthrough(); 5], low_cut: None, high_cut: None }
    }

    pub fn set(&mut self, gains_db: &[f32; 5], low_cut_hz: f32, high_cut_hz: f32) {
        for ((band, &freq), &gain) in self.bands.iter_mut().zip(&EQ_FREQS).zip(gains_db) {
            band.retune(Biquad::peaking(self.sr, freq, 1.1, gain));
        }
        // dois biquads de Butterworth em série: 12 dB/oitava sem pico na quina
        Self::tune(&mut self.low_cut, (low_cut_hz > 20.5).then(|| Biquad::highpass(self.sr, low_cut_hz, 0.707)));
        Self::tune(&mut self.high_cut, (high_cut_hz < 19500.0).then(|| Biquad::lowpass(self.sr, high_cut_hz, 0.707)));
    }

    fn tune(slot: &mut Option<[Biquad; 2]>, want: Option<Biquad>) {
        match (slot.as_mut(), want) {
            (Some(pair), Some(b)) => pair.iter_mut().for_each(|f| f.retune(b)),
            (None, Some(b)) => *slot = Some([b, b]),
            (_, None) => *slot = None,
        }
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for band in &mut self.bands {
            band.process(buf);
        }
        for f in self.low_cut.iter_mut().chain(self.high_cut.iter_mut()).flatten() {
            f.process(buf);
        }
    }
}
