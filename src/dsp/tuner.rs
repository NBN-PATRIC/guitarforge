use serde::{Deserialize, Serialize};

const LETTERS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const SOLFEGE: [&str; 12] = ["Dó", "Dó#", "Ré", "Ré#", "Mi", "Fá", "Fá#", "Sol", "Sol#", "Lá", "Lá#", "Si"];

pub const MIN_STRINGS: usize = 4;
pub const MAX_STRINGS: usize = 9;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Notation {
    #[default]
    Solfege,
    Letters,
}

impl Notation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Solfege => "Dó Ré Mi",
            Self::Letters => "C D E",
        }
    }

    pub fn name(self, index: usize) -> &'static str {
        match self {
            Self::Solfege => SOLFEGE[index % 12],
            Self::Letters => LETTERS[index % 12],
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Solfege => Self::Letters,
            Self::Letters => Self::Solfege,
        }
    }

    /// Nota MIDI por extenso com oitava: 40 vira "Mi2" ou "E2".
    pub fn midi(self, midi: i32) -> String {
        format!("{}{}", self.name(midi.rem_euclid(12) as usize), midi.div_euclid(12) - 1)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Instrument {
    #[default]
    Guitar,
    Bass,
}

impl Instrument {
    pub const ALL: [Instrument; 2] = [Self::Guitar, Self::Bass];

    pub fn label(self) -> &'static str {
        match self {
            Self::Guitar => "Guitarra",
            Self::Bass => "Baixo",
        }
    }
}

/// Afinação em notas MIDI, da corda mais grave para a mais aguda. `notes` é livre: artista, música, instrumento.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tuning {
    pub name: String,
    pub instrument: Instrument,
    pub midi: Vec<i32>,
    pub notes: String,
    #[serde(skip)]
    pub custom: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { name: "Padrão".into(), instrument: Instrument::Guitar, midi: vec![40, 45, 50, 55, 59, 64], notes: String::new(), custom: false }
    }
}

impl Tuning {
    /// "Guitarra 6 cordas", "Baixo 4 cordas".
    pub fn group(&self) -> String {
        format!("{} {} cordas", self.instrument.label(), self.midi.len())
    }

    /// Identificador estável (o mesmo nome pode existir em grupos diferentes, como "Drop D").
    pub fn id(&self) -> String {
        format!("{} · {}", self.group(), self.name)
    }

    /// "Mi Lá Ré Sol Si Mi" ou "E A D G B E".
    pub fn note_names(&self, notation: Notation) -> String {
        self.midi.iter().map(|&m| notation.name(m.rem_euclid(12) as usize)).collect::<Vec<_>>().join(" ")
    }

    /// Número da corda (1 = mais aguda) para o índice `i` (0 = mais grave).
    pub fn string_number(&self, i: usize) -> usize {
        self.midi.len() - i
    }

    pub fn lowest_freq(&self) -> f32 {
        self.midi.iter().copied().min().map(midi_freq).unwrap_or(82.41)
    }
}

fn t(name: &str, instrument: Instrument, midi: &[i32]) -> Tuning {
    Tuning { name: name.into(), instrument, midi: midi.to_vec(), notes: String::new(), custom: false }
}

/// Afinações que já vêm no app. As do usuário ficam em `afinacoes.json` (ver `preset`).
pub fn builtin() -> Vec<Tuning> {
    use Instrument::{Bass, Guitar};
    vec![
        t("Padrão", Guitar, &[40, 45, 50, 55, 59, 64]),
        t("Meio tom abaixo", Guitar, &[39, 44, 49, 54, 58, 63]),
        t("Um tom abaixo", Guitar, &[38, 43, 48, 53, 57, 62]),
        t("Drop D", Guitar, &[38, 45, 50, 55, 59, 64]),
        t("Drop C#", Guitar, &[37, 44, 49, 54, 58, 63]),
        t("Drop C", Guitar, &[36, 43, 48, 53, 57, 62]),
        t("Drop B", Guitar, &[35, 42, 47, 52, 56, 61]),
        t("Duplo Drop D", Guitar, &[38, 45, 50, 55, 59, 62]),
        t("DADGAD", Guitar, &[38, 45, 50, 55, 57, 62]),
        t("Open G", Guitar, &[38, 43, 50, 55, 59, 62]),
        t("Open D", Guitar, &[38, 45, 50, 54, 57, 62]),
        t("Open E", Guitar, &[40, 47, 52, 56, 59, 64]),
        t("Open C", Guitar, &[36, 43, 48, 55, 60, 64]),
        t("Open A", Guitar, &[40, 45, 52, 57, 61, 64]),
        t("Padrão", Guitar, &[35, 40, 45, 50, 55, 59, 64]),
        t("Drop A", Guitar, &[33, 40, 45, 50, 55, 59, 64]),
        t("Padrão", Guitar, &[30, 35, 40, 45, 50, 55, 59, 64]),
        t("Drop E", Guitar, &[28, 35, 40, 45, 50, 55, 59, 64]),
        t("Padrão", Bass, &[28, 33, 38, 43]),
        t("Meio tom abaixo", Bass, &[27, 32, 37, 42]),
        t("Drop D", Bass, &[26, 33, 38, 43]),
        t("Um tom abaixo", Bass, &[26, 31, 36, 41]),
        t("Drop C", Bass, &[24, 31, 36, 41]),
        t("Padrão (Si grave)", Bass, &[23, 28, 33, 38, 43]),
        t("Dó agudo", Bass, &[28, 33, 38, 43, 48]),
        t("Padrão", Bass, &[23, 28, 33, 38, 43, 48]),
    ]
}

pub fn midi_freq(midi: i32) -> f32 {
    440.0 * 2f32.powf((midi - 69) as f32 / 12.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub index: usize,
    pub octave: i32,
    pub cents: f32,
    pub freq: f32,
}

pub fn note_for(freq: f32) -> Note {
    let midi = 69.0 + 12.0 * (freq / 440.0).log2();
    let nearest = midi.round();
    let n = nearest as i32;
    Note { index: n.rem_euclid(12) as usize, octave: n.div_euclid(12) - 1, cents: (midi - nearest) * 100.0, freq }
}

pub fn cents_between(freq: f32, reference: f32) -> f32 {
    1200.0 * (freq / reference).log2()
}

/// Corda que está sendo afinada (0 = a mais grave). Fica na mesma até 4 semitons de distância, desde que nenhuma
/// outra esteja 1 semitom mais perto: assim uma corda frouxa não "vira" a vizinha no meio do ajuste.
pub fn pick_string(freq: f32, tuning: &Tuning, current: Option<usize>) -> Option<usize> {
    let cents: Vec<f32> = tuning.midi.iter().map(|&m| cents_between(freq, midi_freq(m)).abs()).collect();
    let best = (0..cents.len()).min_by(|&a, &b| cents[a].total_cmp(&cents[b]))?;
    if cents[best] > 600.0 {
        return None;
    }
    match current {
        Some(c) if c < cents.len() && cents[c] < 400.0 && cents[c] - cents[best] < 100.0 => Some(c),
        _ => Some(best),
    }
}

/// Corda atual do modo automático. Enquanto a nota desliza aos poucos (tarraxa girando) a corda não muda,
/// mesmo que a nota passe a ficar mais perto da vizinha: foi assim que a 6ª acabou esticada até o Lá, com o
/// afinador dizendo "5ª corda, aperte". Só troca de corda com uma nota nova (salto ou depois de uma pausa).
#[derive(Default)]
pub struct StringTracker {
    current: Option<usize>,
    last_hz: f32,
    last_t: f64,
}

impl StringTracker {
    /// `t` em segundos, de um relógio qualquer que só anda para a frente.
    pub fn update(&mut self, hz: f32, t: f64, tuning: &Tuning) -> Option<usize> {
        let gliding = self.last_hz > 0.0 && t - self.last_t < 0.25 && cents_between(hz, self.last_hz).abs() < 80.0;
        self.last_hz = hz;
        self.last_t = t;
        self.current = match self.current {
            Some(c) if gliding && c < tuning.midi.len() => Some(c),
            c => pick_string(hz, tuning, c),
        };
        self.current
    }

    pub fn set(&mut self, string: Option<usize>) {
        self.current = string;
    }
}

/// Faixa em volta da corda-alvo onde a busca dirigida procura: cobre corda bem frouxa (9 semitons abaixo) e
/// esticada demais (7 acima), mas deixa de fora a oitava de cima e de baixo, onde moram os erros de harmônico.
pub const TARGET_BELOW: f32 = 9.0;
pub const TARGET_ABOVE: f32 = 7.0;

/// Altura da nota, com a corda-alvo como referência quando o usuário escolheu uma.
/// Sem alvo é a busca livre. Com alvo, roda também a busca dirigida (faixa em volta da corda). Se as duas
/// discordam e uma é harmônica da outra, o sinal é ambíguo pelo período; decide a energia: se existe
/// fundamental de verdade na mais grave, é ela (corda grave com fundamental fraca, a busca livre pegou um
/// harmônico); se não existe, a mais grave era só um múltiplo do período e vale a mais aguda (outra corda).
pub fn detect(buf: &[f32], sr: f32, fmin: f32, fmax: f32, target: Option<f32>) -> Option<f32> {
    let free = yin(buf, sr, fmin, fmax);
    let Some(t) = target else { return free };
    let lo = t * 2f32.powf(-TARGET_BELOW / 12.0);
    let hi = t * 2f32.powf(TARGET_ABOVE / 12.0);
    let directed = yin(buf, sr, lo, hi);
    match (free, directed) {
        (Some(f), Some(d)) => Some(choose(buf, sr, f, d)),
        (f, d) => f.or(d),
    }
}

fn choose(buf: &[f32], sr: f32, free: f32, directed: f32) -> f32 {
    let near = |a: f32, b: f32, tol: f32| cents_between(a, b).abs() < tol;
    if near(free, directed, 50.0) {
        return free;
    }
    let (low, high) = if free < directed { (free, directed) } else { (directed, free) };
    if !(2..=6).any(|k| near(high, low * k as f32, 35.0)) {
        return free;
    }
    // fundamental fraca mas presente fica uns 10 a 26 dB abaixo do harmônico forte; ausente, cai no ruído
    let ratio = tone_power(buf, sr, low) / tone_power(buf, sr, high).max(1e-20);
    if ratio > 1e-3 { low } else { high }
}

/// Potência de uma frequência no trecho (Goertzel com janela de Hann).
fn tone_power(buf: &[f32], sr: f32, freq: f32) -> f64 {
    let n = buf.len().max(1) as f64;
    let coeff = 2.0 * (2.0 * std::f64::consts::PI * freq as f64 / sr as f64).cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for (i, &x) in buf.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n).cos();
        let s = x as f64 * w + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0)
}

/// Detector de altura YIN (de Cheveigné e Kawahara, 2002).
pub fn yin(buf: &[f32], sr: f32, fmin: f32, fmax: f32) -> Option<f32> {
    let w = buf.len() / 2;
    let tau_min = ((sr / fmax).floor() as usize).max(2);
    let tau_max = ((sr / fmin).ceil() as usize).min(w.saturating_sub(2));
    if tau_min + 2 >= tau_max {
        return None;
    }

    let mut diff = vec![0.0f32; tau_max + 2];
    for (tau, d) in diff.iter_mut().enumerate().skip(1) {
        *d = buf[..w].iter().zip(&buf[tau..tau + w]).map(|(a, b)| (a - b) * (a - b)).sum();
    }

    let mut cmnd = vec![1.0f32; tau_max + 2];
    let mut running = 0.0;
    for tau in 1..tau_max + 2 {
        running += diff[tau];
        cmnd[tau] = if running > 0.0 { diff[tau] * tau as f32 / running } else { 1.0 };
    }

    // O YIN clássico pega o primeiro vale abaixo de um limite fixo (0,1 a 0,15). Na corda grave com fundamental
    // fraca isso cai num vale falso antes do período verdadeiro: no cabo real o Mi2 virava 4/3 dele (Lá2) ou o
    // 4º harmônico. Aqui o limite é relativo ao vale mais fundo (ideia do MPM de McLeod): só vale um candidato
    // quase tão bom quanto o melhor, e o primeiro desses ainda evita o erro de oitava para baixo.
    const MAX_APERIODICITY: f32 = 0.2;
    let best = cmnd[tau_min..=tau_max].iter().copied().fold(f32::INFINITY, f32::min);
    if best > MAX_APERIODICITY {
        return None;
    }
    let threshold = (best * 1.5).max(best + 0.02).min(MAX_APERIODICITY);
    let mut tau = tau_min;
    while tau <= tau_max && cmnd[tau] >= threshold {
        tau += 1;
    }
    while tau < tau_max && cmnd[tau + 1] < cmnd[tau] {
        tau += 1;
    }

    let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[tau + 1]);
    let denom = a - 2.0 * b + c;
    let offset = if denom.abs() > 1e-9 { ((a - c) / (2.0 * denom)).clamp(-1.0, 1.0) } else { 0.0 };
    Some(sr / (tau as f32 + offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::f32::consts::PI;

    fn instrument_like(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                (1..=6).map(|h| (2.0 * PI * freq * h as f32 * t).sin() / h as f32).sum::<f32>() * 0.2
            })
            .collect()
    }

    fn by_id(id: &str) -> Tuning {
        builtin().into_iter().find(|t| t.id() == id).expect(id)
    }

    #[test]
    fn note_names_and_cents() {
        let a4 = note_for(440.0);
        assert_eq!((Notation::Letters.name(a4.index), a4.octave), ("A", 4));
        assert_eq!(Notation::Solfege.name(a4.index), "Lá");
        assert!(a4.cents.abs() < 0.01);
        let e2 = note_for(82.17);
        assert_eq!((Notation::Solfege.name(e2.index), e2.octave), ("Mi", 2));
        assert!((e2.cents + 5.0).abs() < 1.0);
        assert_eq!(Notation::Solfege.midi(23), "Si0");
        assert_eq!(Tuning::default().note_names(Notation::Solfege), "Mi Lá Ré Sol Si Mi");
        assert_eq!(by_id("Guitarra 6 cordas · Drop D").note_names(Notation::Letters), "D A D G B E");
        assert_eq!(by_id("Baixo 4 cordas · Padrão").note_names(Notation::Letters), "E A D G");
    }

    #[test]
    fn builtin_ids_are_unique_and_sizes_are_valid() {
        let list = builtin();
        let ids: HashSet<String> = list.iter().map(|t| t.id()).collect();
        assert_eq!(ids.len(), list.len());
        assert!(list.iter().all(|t| (MIN_STRINGS..=MAX_STRINGS).contains(&t.midi.len())));
        assert!(list.iter().all(|t| t.midi.windows(2).all(|w| w[0] < w[1] || t.name.starts_with("Open"))));
    }

    #[test]
    fn string_detection_sticks_to_the_string_being_tuned() {
        let std = Tuning::default();
        assert_eq!(pick_string(108.8, &std, None), Some(1));
        assert_eq!(std.string_number(1), 5);
        // 5ª corda (Lá) 280 cents abaixo: já está mais perto do Mi da 6ª, mas quem estava sendo afinada era a 5ª
        let slack_a = midi_freq(45) * 2f32.powf(-2.8 / 12.0);
        assert_eq!(pick_string(slack_a, &std, Some(1)), Some(1));
        assert_eq!(pick_string(slack_a, &std, None), Some(0));
        assert_eq!(pick_string(1500.0, &std, None), None);
        assert_eq!(pick_string(73.4, &by_id("Guitarra 6 cordas · Drop D"), None), Some(0));
        let bass5 = by_id("Baixo 5 cordas · Padrão (Si grave)");
        assert_eq!(pick_string(31.0, &bass5, None), Some(0));
        assert_eq!(bass5.string_number(0), 5);
        // histórico de uma afinação com mais cordas não pode estourar o índice
        assert_eq!(pick_string(41.2, &by_id("Baixo 4 cordas · Padrão"), Some(7)), Some(0));
    }

    #[test]
    fn turning_the_peg_never_jumps_to_the_neighbor_string() {
        let std = Tuning::default();
        let mut tr = StringTracker::default();
        let mut t = 0.0;
        // 6ª corda solta, depois a tarraxa girando para cima até 113 Hz (o que aconteceu no cabo real)
        let mut hz = 82.4f32;
        while hz < 113.5 {
            assert_eq!(tr.update(hz, t, &std), Some(0), "{hz} Hz saiu da 6ª corda");
            hz *= 2f32.powf(0.3 / 12.0);
            t += 1.0 / 30.0;
        }
        // pausa e nota nova: 5ª corda de verdade
        t += 1.0;
        assert_eq!(tr.update(110.0, t, &std), Some(1));
        // salto direto para a 4ª, sem pausa
        t += 1.0 / 30.0;
        assert_eq!(tr.update(146.8, t, &std), Some(2));
    }

    #[test]
    fn yin_finds_guitar_and_bass_strings() {
        let sr = 24000.0;
        let cases = [(&Tuning::default(), 2048usize, 60.0f32), (&by_id("Baixo 5 cordas · Padrão (Si grave)"), 4096, 22.0)];
        for (tuning, len, fmin) in cases {
            for &m in &tuning.midi {
                let f0 = midi_freq(m);
                let sig = instrument_like(f0, sr, len);
                let f = yin(&sig, sr, fmin, 1100.0).expect("sem altura");
                assert!(cents_between(f, f0).abs() < 3.0, "{f0} -> {f}");
            }
        }
    }

    /// Mi grave de single coil na ponte: fundamental fraca, 4º harmônico dominante (erro visto no cabo real: 108 Hz).
    fn weak_fundamental(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        let amps = [0.05, 0.12, 0.1, 1.0, 0.08, 0.06, 0.05, 0.35];
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                amps.iter().enumerate().map(|(h, a)| a * (2.0 * PI * freq * (h + 1) as f32 * t + h as f32).sin()).sum::<f32>() * 0.1
            })
            .collect()
    }

    #[test]
    fn yin_low_e_with_weak_fundamental() {
        let sr = 24000.0;
        for f0 in [82.17f32, 82.41, 110.0, 146.83] {
            let sig = weak_fundamental(f0, sr, 2048);
            let f = yin(&sig, sr, 60.0, 1100.0).expect("sem altura");
            assert!(cents_between(f, f0).abs() < 5.0, "{f0} -> {f}");
        }
    }

    #[test]
    fn selected_string_ignores_harmonic_locking() {
        let sr = 24000.0;
        // 6ª corda meio tom abaixo (80 Hz) com o 4º harmônico dominante (no cabo real a busca livre leu 320 Hz)
        let sig = weak_fundamental(80.0, sr, 4096);
        let f = detect(&sig, sr, 60.0, 1100.0, Some(midi_freq(40))).unwrap();
        assert!(cents_between(f, 80.0).abs() < 5.0, "com a 6ª escolhida: {f}");
        // corda esticada demais (113 Hz, +5 semitons) continua sendo lida como ela é
        let tight = instrument_like(113.5, sr, 4096);
        let f = detect(&tight, sr, 60.0, 1100.0, Some(midi_freq(40))).unwrap();
        assert!(cents_between(f, 113.5).abs() < 5.0, "esticada: {f}");
        // outra corda tocada (Ré3 com a 6ª escolhida) aparece como ela é, não como harmônico
        let other = instrument_like(146.83, sr, 4096);
        let f = detect(&other, sr, 60.0, 1100.0, Some(midi_freq(40))).unwrap();
        assert!(cents_between(f, 146.83).abs() < 5.0, "outra corda: {f}");
    }

    #[test]
    fn octave_down_error_is_not_trusted_without_a_real_fundamental() {
        let sr = 24000.0;
        // Sol3 tocado com a 6ª escolhida: a busca dirigida acha Sol2 (dois períodos), mas lá não há energia
        let g3 = instrument_like(196.0, sr, 4096);
        let f = detect(&g3, sr, 60.0, 1100.0, Some(midi_freq(40))).unwrap();
        assert!(cents_between(f, 196.0).abs() < 5.0, "Sol3: {f}");
    }

    #[test]
    fn yin_rejects_noise() {
        let mut x = 12345u32;
        let noise: Vec<f32> = (0..2048)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32) - 0.5
            })
            .collect();
        assert!(yin(&noise, 24000.0, 60.0, 1100.0).is_none());
    }

    #[test]
    fn custom_tuning_roundtrip() {
        let mut t = Tuning { name: "Minha".into(), instrument: Instrument::Bass, midi: vec![26, 33, 38, 43], ..Default::default() };
        t.notes = "música X".into();
        let json = serde_json::to_string(&t).unwrap();
        let back: Tuning = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id(), "Baixo 4 cordas · Minha");
        assert_eq!(back.notes, "música X");
    }
}
