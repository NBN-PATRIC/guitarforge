use crate::dsp::biquad::Biquad;
use crate::dsp::db_to_lin;
use crate::settings::{AmpCfg, AmpChannel};

/// Pré com 1 ou 2 estágios de saturação, tonestack depois do pré (como nos amps
/// valvulados) e saturação suave de power amp. Controles de tom neutros no meio.
pub struct AmpSim {
    sr: f32,
    input_hp: Biquad,
    pre_lp: Biquad,
    inter_lp: Biquad,
    dc_block: Biquad,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    presence: Biquad,
    pre_gain: f32,
    bias: f32,
    two_stages: bool,
    power_gain: f32,
    out_gain: f32,
}

impl AmpSim {
    pub fn new(sr: f32) -> Self {
        let mut a = Self {
            sr,
            input_hp: Biquad::passthrough(),
            pre_lp: Biquad::passthrough(),
            inter_lp: Biquad::passthrough(),
            dc_block: Biquad::passthrough(),
            bass: Biquad::passthrough(),
            mid: Biquad::passthrough(),
            treble: Biquad::passthrough(),
            presence: Biquad::passthrough(),
            pre_gain: 1.0,
            bias: 0.0,
            two_stages: false,
            power_gain: 1.0,
            out_gain: 1.0,
        };
        a.set(&AmpCfg::default());
        a
    }

    pub fn set(&mut self, c: &AmpCfg) {
        let sr = self.sr;
        // ganho final de cada canal: com os knobs no padrão os três soam iguais (medido com o acorde de referência,
        // teste `amp_channels_sound_equally_loud`); até a v0.4 o Lead saía 12 dB acima do Limpo
        let (hp_hz, gain_db, bias, two_stages, out_gain) = match c.channel {
            AmpChannel::Clean => (40.0, -6.0 + c.gain * 18.0, 0.0, false, 3.3),
            AmpChannel::Crunch => (90.0, 6.0 + c.gain * 24.0, 0.12, false, 1.0),
            AmpChannel::Lead => (130.0, 14.0 + c.gain * 26.0, 0.18, true, 0.43),
        };
        self.input_hp.retune(Biquad::highpass(sr, hp_hz, 0.707));
        self.pre_lp.retune(Biquad::lowpass(sr, 9000.0, 0.707));
        self.inter_lp.retune(Biquad::lowpass(sr, 5500.0, 0.707));
        self.dc_block.retune(Biquad::highpass(sr, 15.0, 0.707));
        self.bass.retune(Biquad::low_shelf(sr, 120.0, (c.bass - 0.5) * 24.0));
        self.mid.retune(Biquad::peaking(sr, 700.0, 0.8, (c.mid - 0.5) * 24.0));
        self.treble.retune(Biquad::high_shelf(sr, 2800.0, (c.treble - 0.5) * 24.0));
        self.presence.retune(Biquad::high_shelf(sr, 5000.0, c.presence * 10.0 - 3.0));
        self.pre_gain = db_to_lin(gain_db);
        self.bias = bias;
        self.two_stages = two_stages;
        self.power_gain = 0.4 + c.volume * 2.0;
        self.out_gain = out_gain;
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let offset1 = self.bias.tanh();
        let bias2 = self.bias * 0.5;
        let offset2 = bias2.tanh();
        for s in buf.iter_mut() {
            let mut x = self.pre_lp.tick(self.input_hp.tick(*s)) * self.pre_gain;
            x = (x + self.bias).tanh() - offset1;
            if self.two_stages {
                x = self.inter_lp.tick(x) * 4.0;
                x = (x + bias2).tanh() - offset2;
            }
            x = self.dc_block.tick(x);
            x = self.presence.tick(self.treble.tick(self.mid.tick(self.bass.tick(x))));
            x *= self.power_gain;
            x /= 1.0 + x.abs();
            *s = x * self.out_gain;
        }
    }
}
