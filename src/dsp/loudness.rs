//! Volume igual entre presets: quanto um preset soa alto, medido do jeito que o ouvido percebe.
//!
//! Um acorde de referência (Mi maior, 4 batidas, no nível de um DI real do cabo) passa pela cadeia do preset fora do
//! áudio, a 48 kHz e sem sobreamostragem; a saída estéreo é medida com o filtro K da ITU-R BS.1770 (o mesmo do LUFS:
//! tira o grave que o ouvido quase não sente e realça o agudo de presença) em blocos de 400 ms, ignorando os blocos
//! 10 dB abaixo da média (o fim do rabo não puxa a conta). A compensação é a diferença para `TARGET`.

use crate::dsp::SignalChain;
use crate::settings::Settings;

/// Volume de chegada de todos os presets (dB K-ponderado, escala da BS.1770), com o knob de volume no máximo: o do
/// "Limpo cristalino", o preset de abertura, que assim não muda.
pub const TARGET: f32 = -11.5;
/// A medida roda com o volume em 0,5 (12 dB abaixo do máximo) para o limitador do fim não entrar na conta.
const MEASURE_MASTER: f32 = 0.5;
/// Compensação máxima, para cima e para baixo.
pub const MAX_TRIM_DB: f32 = 24.0;
const SR: f32 = 48000.0;

/// Acorde de Mi maior aberto (6 cordas, com harmônicos), quatro batidas de 0,6 s. O bloco de 20 ms mais forte fica
/// em -21 dBFS, como o acorde forte do DI gravado em 01/11.
pub fn reference() -> Vec<f32> {
    let strings = [82.41f32, 123.47, 164.81, 207.65, 246.94, 329.63];
    let n = (SR * 2.6) as usize;
    let mut x = vec![0.0f32; n];
    for strum in 0..4 {
        let start = (strum as f32 * 0.6 * SR) as usize;
        for (k, &f) in strings.iter().enumerate() {
            // a palheta passa pelas cordas em 15 ms
            let at = start + (k as f32 * 0.003 * SR) as usize;
            for (i, v) in x[at..].iter_mut().enumerate() {
                let t = i as f32 / SR;
                let env = (-t * 3.0).exp();
                let s: f32 = (1..7)
                    .map(|h| 1.0 / h as f32 * (2.0 * std::f32::consts::PI * f * h as f32 * t + k as f32).sin())
                    .sum();
                *v += s * env;
            }
        }
    }
    let block = (SR * 0.02) as usize;
    let loudest = x
        .chunks(block)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .fold(0.0f32, f32::max);
    let gain = 10f32.powf(-21.0 / 20.0) / loudest;
    x.iter_mut().for_each(|v| *v *= gain);
    x
}

/// Filtro K da BS.1770 a 48 kHz (prateleira de agudo + passa-altas RLB), coeficientes da norma.
struct KWeight {
    s: [[f32; 2]; 2],
}

impl KWeight {
    const STAGES: [([f32; 3], [f32; 2]); 2] = [
        ([1.535_124_9, -2.691_696_2, 1.198_392_8], [-1.690_659_3, 0.732_480_8]),
        ([1.0, -2.0, 1.0], [-1.990_047_5, 0.990_072_25]),
    ];

    fn tick(&mut self, mut x: f32) -> f32 {
        for (st, (b, a)) in self.s.iter_mut().zip(Self::STAGES) {
            // forma direta II transposta
            let y = b[0] * x + st[0];
            st[0] = b[1] * x - a[0] * y + st[1];
            st[1] = b[2] * x - a[1] * y;
            x = y;
        }
        x
    }
}

/// Volume percebido de um sinal estéreo (dB K-ponderado; -0,691 + 10 log10 da média, como a BS.1770).
pub fn measure(left: &[f32], right: &[f32]) -> f32 {
    let (mut kl, mut kr) = (KWeight { s: [[0.0; 2]; 2] }, KWeight { s: [[0.0; 2]; 2] });
    let block = (SR * 0.4) as usize;
    let mut powers = Vec::new();
    for (l, r) in left.chunks(block).zip(right.chunks(block)) {
        let sum: f32 = l.iter().zip(r).map(|(&a, &b)| kl.tick(a).powi(2) + kr.tick(b).powi(2)).sum();
        powers.push(sum / l.len() as f32);
    }
    let lufs = |p: f32| -0.691 + 10.0 * p.max(1e-12).log10();
    let mean = powers.iter().sum::<f32>() / powers.len().max(1) as f32;
    let gated: Vec<f32> = powers.iter().copied().filter(|&p| lufs(p) > lufs(mean) - 10.0).collect();
    lufs(gated.iter().sum::<f32>() / gated.len().max(1) as f32)
}

/// Volume do preset com o acorde de referência. Os ajustes globais (volume, mudo, nível de entrada, gate) ficam de
/// fora: volume no máximo, sem gate, sem compensação.
pub fn of_settings(settings: &Settings) -> f32 {
    with_trim(settings, 0.0)
}

/// O mesmo, com a compensação `level_db` aplicada pela própria cadeia (inclui o limitador do fim).
fn with_trim(settings: &Settings, level_db: f32) -> f32 {
    let mut s = *settings;
    s.master = MEASURE_MASTER;
    s.mute = false;
    s.input_gain_db = 0.0;
    s.level_db = level_db;
    s.gate.on = false;
    s.gate.hum_filter = false;
    let mut chain = SignalChain::new(SR, &s, 1);
    let mut left = reference();
    let mut right = vec![0.0; left.len()];
    for (l, r) in left.chunks_mut(256).zip(right.chunks_mut(256)) {
        chain.process_stereo(l, r);
    }
    measure(&left, &right) - 40.0 * MEASURE_MASTER.log10()
}

/// Compensação (dB) que leva o preset ao `TARGET`.
pub fn trim_for(settings: &Settings) -> f32 {
    (TARGET - of_settings(settings)).clamp(-MAX_TRIM_DB, MAX_TRIM_DB)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset;
    use crate::settings::AmpChannel;

    /// Seno de 1 kHz a -20 dBFS nos dois lados: a BS.1770 dá -20 + 3 (dois canais) - 0,691 + ~0,7 do filtro K.
    #[test]
    fn sine_matches_the_standard() {
        let x: Vec<f32> = (0..48000 * 3).map(|i| 0.1 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR).sin()).collect();
        let l = measure(&x, &x);
        assert!((l + 20.0).abs() < 0.3, "{l}");
    }

    /// Imprime o volume cru de cada preset de fábrica e de cada canal do amp:
    /// `cargo test --release -- --ignored --nocapture volumes`.
    #[test]
    #[ignore]
    fn volumes() {
        for p in preset::factory() {
            println!("{:22} {:6.1} dB", p.name, of_settings(&p.settings));
        }
        for ch in AmpChannel::ALL {
            let mut s = Settings::default();
            s.amp.channel = ch;
            println!("canal {:?}: {:6.1} dB", ch, of_settings(&s));
        }
    }

    /// Trocar de canal no meio de um preset não pode pular de volume (com os knobs no padrão).
    #[test]
    fn amp_channels_sound_equally_loud() {
        let levels: Vec<f32> = AmpChannel::ALL
            .iter()
            .map(|&ch| {
                let mut s = Settings::default();
                s.amp.channel = ch;
                of_settings(&s)
            })
            .collect();
        let (lo, hi) = levels.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(hi - lo < 1.5, "canais Limpo, Crunch e Lead: {levels:?} dB");
    }

    /// Com a compensação passando pela cadeia de verdade (e pelo limitador do fim), todo preset de fábrica chega ao
    /// mesmo volume com o knob no máximo.
    #[test]
    fn every_factory_preset_lands_on_the_target() {
        for p in preset::factory() {
            let trim = trim_for(&p.settings);
            let after = with_trim(&p.settings, trim);
            assert!((after - TARGET).abs() < 1.0, "{}: compensação {trim:+.1} dB, chegou a {after:.1} dB", p.name);
        }
    }
}
