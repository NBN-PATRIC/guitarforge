use std::f64::consts::PI;

/// Nível RMS (dBFS) de uma única frequência no trecho, por Goertzel.
/// Com janela de 1 s a 60 Hz cabem ciclos inteiros, então a ordem circular do buffer não importa.
pub fn tone_level_db(buf: &[f32], sr: f32, freq: f32) -> f32 {
    if buf.is_empty() {
        return -120.0;
    }
    let coeff = 2.0 * (2.0 * PI * freq as f64 / sr as f64).cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in buf {
        let s = x as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    let power = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0);
    let amplitude = 2.0 * power.sqrt() / buf.len() as f64;
    let rms = amplitude / std::f64::consts::SQRT_2;
    (20.0 * rms.max(1e-6).log10()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_60hz_level() {
        let sr = 48000.0;
        let amp = 0.01f32;
        let buf: Vec<f32> = (0..48000)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 60.0 * i as f32 / sr).sin()
                + 0.02 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr).sin())
            .collect();
        let expected = 20.0 * (amp / std::f32::consts::SQRT_2).log10();
        assert!((tone_level_db(&buf, sr, 60.0) - expected).abs() < 0.3);
    }
}
