pub mod amp;
pub mod biquad;
pub mod cabinet;
pub mod effects;
pub mod hum_filter;
pub mod hum_meter;
pub mod loudness;
pub mod noise_gate;
pub mod oversample;
pub mod room;
pub mod tuner;

use crate::settings::Settings;

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Liga, na thread atual, o modo do processador que zera números minúsculos (denormais, abaixo de 1e-38) em vez
/// de calculá-los devagar. Sem isso, o rabo do reverb, dos filtros e do gate fechando passa segundos nessa faixa.
/// Vale para a thread inteira e até ela acabar: chamar no começo de cada callback de áudio é barato.
pub fn flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: só lê e grava o registrador de controle do SSE da própria thread (bits FTZ e DAZ).
    unsafe {
        let mut csr: u32 = 0;
        std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr, options(nostack, preserves_flags));
        csr |= 0x8040;
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
    }
}

/// Coeficiente de um filtro de um polo com constante de tempo `ms`.
pub fn time_coef(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms * 0.001 * sr)).exp()
}

struct Smoothed {
    current: f32,
    target: f32,
    coef: f32,
}

impl Smoothed {
    fn new(value: f32, sr: f32) -> Self {
        Self { current: value, target: value, coef: 1.0 - time_coef(20.0, sr) }
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coef;
        self.current
    }
}

/// Transição de 10 ms entre o som sem e com um estágio: pisar no pedal no meio da nota não estala.
struct Fade {
    on: bool,
    mix: f32,
    step: f32,
}

impl Fade {
    fn new(sr: f32) -> Self {
        Self { on: false, mix: 0.0, step: 1.0 / (0.010 * sr) }
    }

    /// Devolve `true` quando o estágio estava calado e vai voltar a soar: hora de limpar a memória dele, para não
    /// sair o resto do som de quando foi desligado.
    fn set(&mut self, on: bool) -> bool {
        let waking = on && !self.on && self.mix == 0.0;
        self.on = on;
        waking
    }

    /// Vai direto ao estado pedido, sem transição (cadeia recém-criada).
    fn settle(&mut self) {
        self.mix = if self.on { 1.0 } else { 0.0 };
    }
}

/// O mesmo que `stage`, para os estágios estéreo (o fim da cadeia).
fn stage2(
    fade: &mut Fade,
    dry: (&mut Vec<f32>, &mut Vec<f32>),
    left: &mut [f32],
    right: &mut [f32],
    f: impl FnOnce(&mut [f32], &mut [f32]),
) {
    if fade.on && fade.mix >= 1.0 {
        f(left, right);
        return;
    }
    if !fade.on && fade.mix <= 0.0 {
        return;
    }
    let (dry_l, dry_r) = dry;
    dry_l.clear();
    dry_l.extend_from_slice(left);
    dry_r.clear();
    dry_r.extend_from_slice(right);
    f(left, right);
    let step = if fade.on { fade.step } else { -fade.step };
    for ((l, r), (&dl, &dr)) in left.iter_mut().zip(right.iter_mut()).zip(dry_l.iter().zip(dry_r.iter())) {
        fade.mix = (fade.mix + step).clamp(0.0, 1.0);
        *l = dl + (*l - dl) * fade.mix;
        *r = dr + (*r - dr) * fade.mix;
    }
}

/// Roda o estágio `f` no bloco: inteiro, nada, ou na transição, misturado com o som de antes dele.
fn stage(fade: &mut Fade, dry: &mut Vec<f32>, buf: &mut [f32], f: impl FnOnce(&mut [f32])) {
    if fade.on && fade.mix >= 1.0 {
        f(buf);
        return;
    }
    if !fade.on && fade.mix <= 0.0 {
        return;
    }
    dry.clear();
    dry.extend_from_slice(buf);
    f(buf);
    let step = if fade.on { fade.step } else { -fade.step };
    for (y, &x) in buf.iter_mut().zip(dry.iter()) {
        fade.mix = (fade.mix + step).clamp(0.0, 1.0);
        *y = x + (*y - x) * fade.mix;
    }
}

/// Troca que muda o som de uma vez, sem como misturar o antes e o depois: canal do amp, tipo de drive, gabinete.
/// Essas (e a troca de preset ou de IR) passam por um silêncio curto antes do delay e do reverb.
fn abrupt(a: &Settings, b: &Settings) -> bool {
    let amp = (a.amp.on || b.amp.on) && a.amp.channel != b.amp.channel;
    let drive = (a.drive.on || b.drive.on) && a.drive.kind != b.drive.kind;
    let cab = (a.cab.on || b.cab.on) && (a.cab.kind != b.cab.kind || a.cab.use_ir != b.cab.use_ir);
    amp || drive || cab
}

/// Bloco máximo processado de uma vez (o resto vai em pedaços): a memória da transição é alocada uma vez só.
const MAX_CHUNK: usize = 2048;
/// Quanto o som leva para abaixar (e para voltar) numa troca brusca.
const DUCK_MS: f32 = 5.0;

/// Cadeia fixa: entrada > filtro de 60 Hz > gate > comp > wah > oitava > boost > drive > chorus > phaser > flanger >
/// tremolo > amp > gabinete > EQ > delay > estéreo > reverb > volume. Mono até o delay, estéreo dali em diante.
pub struct SignalChain {
    input_gain: Smoothed,
    master: Smoothed,
    hum_fade: Fade,
    hum: hum_filter::HumFilter,
    floor: noise_gate::FloorTracker,
    auto_gate: bool,
    manual_threshold_db: f32,
    /// Limiar em uso pelo gate automático (para mostrar no knob).
    pub auto_threshold_db: Option<f32>,
    sample_rate: f32,
    gate_fade: Fade,
    gate: noise_gate::NoiseGate,
    comp_fade: Fade,
    comp: effects::Compressor,
    wah_fade: Fade,
    wah: effects::Wah,
    octave_fade: Fade,
    octave: effects::Octave,
    boost_fade: Fade,
    boost: effects::Boost,
    drive_fade: Fade,
    drive: effects::Drive,
    drive_os: oversample::Oversampler,
    chorus_fade: Fade,
    chorus: effects::Chorus,
    phaser_fade: Fade,
    phaser: effects::Phaser,
    flanger_fade: Fade,
    flanger: effects::Flanger,
    tremolo_fade: Fade,
    tremolo: effects::Tremolo,
    amp_fade: Fade,
    amp: amp::AmpSim,
    amp_os: oversample::Oversampler,
    cab_fade: Fade,
    pub cabinet: cabinet::CabinetSim,
    eq_fade: Fade,
    eq: effects::ParametricEq,
    delay_fade: Fade,
    delay: effects::Delay,
    spread_fade: Fade,
    spread: effects::Spread,
    reverb_fade: Fade,
    reverb: effects::Reverb,
    /// Som de antes do estágio que está em transição (e o do lado direito, nos estágios estéreo).
    dry: Vec<f32>,
    dry_r: Vec<f32>,
    /// Ajustes em uso (para saber se a próxima troca é brusca).
    current: Settings,
    /// Troca brusca esperando o som abaixar.
    pending: Option<Settings>,
    /// IR novo (ou a retirada dele) esperando o som abaixar, e o antigo esperando ser recolhido fora do áudio.
    pending_ir: Option<Option<Box<cabinet::Convolver>>>,
    old_ir: Option<Box<cabinet::Convolver>>,
    duck_gain: f32,
    duck_step: f32,
}

impl SignalChain {
    /// `oversample`: 1, 2 ou 4; o drive e o amp rodam nessa taxa multiplicada.
    pub fn new(sr: f32, settings: &Settings, oversample: usize) -> Self {
        let hi = sr * oversample.max(1) as f32;
        let mut chain = Self {
            input_gain: Smoothed::new(1.0, sr),
            master: Smoothed::new(0.0, sr),
            hum_fade: Fade::new(sr),
            hum: hum_filter::HumFilter::new(sr, hum_filter::DEFAULT_MAINS_HZ),
            floor: noise_gate::FloorTracker::new(sr),
            auto_gate: true,
            manual_threshold_db: -52.0,
            auto_threshold_db: None,
            sample_rate: sr,
            gate_fade: Fade::new(sr),
            gate: noise_gate::NoiseGate::new(sr),
            comp_fade: Fade::new(sr),
            comp: effects::Compressor::new(sr),
            wah_fade: Fade::new(sr),
            wah: effects::Wah::new(sr),
            octave_fade: Fade::new(sr),
            octave: effects::Octave::new(sr),
            boost_fade: Fade::new(sr),
            boost: effects::Boost::new(sr),
            drive_fade: Fade::new(sr),
            drive: effects::Drive::new(hi),
            drive_os: oversample::Oversampler::new(oversample),
            chorus_fade: Fade::new(sr),
            chorus: effects::Chorus::new(sr),
            phaser_fade: Fade::new(sr),
            phaser: effects::Phaser::new(sr),
            flanger_fade: Fade::new(sr),
            flanger: effects::Flanger::new(sr),
            tremolo_fade: Fade::new(sr),
            tremolo: effects::Tremolo::new(sr),
            amp_fade: Fade::new(sr),
            amp: amp::AmpSim::new(hi),
            amp_os: oversample::Oversampler::new(oversample),
            cab_fade: Fade::new(sr),
            cabinet: cabinet::CabinetSim::new(sr),
            eq_fade: Fade::new(sr),
            eq: effects::ParametricEq::new(sr),
            delay_fade: Fade::new(sr),
            delay: effects::Delay::new(sr),
            spread_fade: Fade::new(sr),
            spread: effects::Spread::new(sr),
            reverb_fade: Fade::new(sr),
            reverb: effects::Reverb::new(sr),
            dry: Vec::with_capacity(MAX_CHUNK),
            dry_r: Vec::with_capacity(MAX_CHUNK),
            current: *settings,
            pending: None,
            pending_ir: None,
            old_ir: None,
            duck_gain: 1.0,
            duck_step: 1.0 / (DUCK_MS * 0.001 * sr),
        };
        chain.apply_now(settings);
        for f in chain.fades() {
            f.settle();
        }
        chain
    }

    fn fades(&mut self) -> [&mut Fade; 17] {
        [
            &mut self.hum_fade,
            &mut self.gate_fade,
            &mut self.comp_fade,
            &mut self.wah_fade,
            &mut self.octave_fade,
            &mut self.boost_fade,
            &mut self.drive_fade,
            &mut self.chorus_fade,
            &mut self.phaser_fade,
            &mut self.flanger_fade,
            &mut self.tremolo_fade,
            &mut self.amp_fade,
            &mut self.cab_fade,
            &mut self.eq_fade,
            &mut self.delay_fade,
            &mut self.spread_fade,
            &mut self.reverb_fade,
        ]
    }

    /// Ajustes novos. `switch` = troca de preset (ou desfazer): tudo muda de uma vez, então passa pelo silêncio curto;
    /// sem isso, só as trocas bruscas (canal do amp, tipo de drive, gabinete) passam por ele.
    pub fn apply(&mut self, s: &Settings, switch: bool) {
        if self.pending.is_some() || switch || abrupt(&self.current, s) {
            self.pending = Some(*s);
        } else {
            self.apply_now(s);
        }
    }

    /// Troca o IR do gabinete (ou tira, com `None`) no silêncio curto. O antigo sai por `take_old_ir`.
    pub fn queue_ir(&mut self, conv: Option<Box<cabinet::Convolver>>) {
        self.pending_ir = Some(conv);
    }

    /// Ainda há um IR esperando a vez (o próximo fica na fila de quem manda).
    pub fn ir_queued(&self) -> bool {
        self.pending_ir.is_some() || self.old_ir.is_some()
    }

    /// IR trocado, para ser liberado fora da thread de áudio.
    pub fn take_old_ir(&mut self) -> Option<Box<cabinet::Convolver>> {
        self.old_ir.take()
    }

    /// Sala por convolução nova (ou nenhuma); entra em transição com a de agora. Devolve uma para liberar já.
    pub fn swap_room(&mut self, room: Option<Box<room::RoomConvolver>>) -> Option<Box<room::RoomConvolver>> {
        self.reverb.swap_room(room)
    }

    /// Sala que terminou a transição, para ser liberada fora da thread de áudio.
    pub fn take_retired_room(&mut self) -> Option<Box<room::RoomConvolver>> {
        self.reverb.take_retired_room()
    }

    fn apply_now(&mut self, s: &Settings) {
        self.current = *s;
        self.input_gain.target = db_to_lin(s.input_gain_db);
        self.master.target = if s.mute { 0.0 } else { s.master * s.master * db_to_lin(s.level_db) };

        self.hum_fade.set(s.gate.hum_filter);
        self.gate_fade.set(s.gate.on);
        self.auto_gate = s.gate.auto;
        self.manual_threshold_db = s.gate.threshold_db;
        self.gate.set_times(s.gate.hold_ms, s.gate.release_ms, s.gate.range_db);
        if !self.auto_gate {
            self.gate.set_threshold_db(s.gate.threshold_db);
        }
        self.comp_fade.set(s.comp.on);
        self.comp.set(s.comp.sustain, s.comp.level);
        if self.wah_fade.set(s.wah.on) {
            self.wah.clear();
        }
        self.wah.set(s.wah.sens, s.wah.resonance, s.wah.range);
        if self.octave_fade.set(s.octave.on) {
            self.octave.clear();
        }
        self.octave.set(s.octave.sub, s.octave.up, s.octave.dry);
        self.boost_fade.set(s.boost.on);
        self.boost.set(s.boost.level_db, s.boost.bright);
        self.drive_fade.set(s.drive.on);
        self.drive.set(&s.drive);

        if self.chorus_fade.set(s.chorus.on) {
            self.chorus.clear();
        }
        self.chorus.set(s.chorus.rate, s.chorus.depth, s.chorus.mix);
        if self.phaser_fade.set(s.phaser.on) {
            self.phaser.clear();
        }
        self.phaser.set(s.phaser.rate, s.phaser.depth, s.phaser.feedback);
        if self.flanger_fade.set(s.flanger.on) {
            self.flanger.clear();
        }
        self.flanger.set(s.flanger.rate, s.flanger.depth, s.flanger.feedback);
        self.tremolo_fade.set(s.tremolo.on);
        self.tremolo.set(s.tremolo.rate, s.tremolo.depth, s.tremolo.shape);

        self.amp_fade.set(s.amp.on);
        self.amp.set(&s.amp);
        self.cab_fade.set(s.cab.on);
        self.cabinet.set(&s.cab);
        self.eq_fade.set(s.eq.on);
        self.eq.set(&s.eq.bands_db, s.eq.low_cut_hz, s.eq.high_cut_hz);

        self.delay.set(s.delay.time_ms, s.delay.feedback, s.delay.mix);
        if self.delay_fade.set(s.delay.on) {
            self.delay.clear();
        }
        if self.spread_fade.set(s.spread.on) {
            self.spread.clear();
        }
        self.spread.set(s.spread.width);
        if self.reverb_fade.set(s.reverb.on) {
            self.reverb.clear();
        }
        self.reverb.set(s.reverb.size, s.reverb.mix);
        self.reverb.set_room(s.reverb.kind.is_convolution(), s.reverb.pre_delay_ms, self.sample_rate);
    }

    pub fn set_mains(&mut self, hz: f32) {
        self.hum.retune(hz);
    }

    pub fn gate_open(&self) -> bool {
        !self.gate_fade.on || self.gate.open
    }

    /// `left` traz o som do cabo; saem os dois lados em `left` e `right` (do mesmo tamanho).
    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        debug_assert_eq!(left.len(), right.len());
        for (l, r) in left.chunks_mut(MAX_CHUNK).zip(right.chunks_mut(MAX_CHUNK)) {
            self.process_chunk(l, r);
        }
    }

    /// Mono (a média dos dois lados), para os testes. Aloca: fora da thread de áudio.
    #[cfg(test)]
    pub fn process(&mut self, buf: &mut [f32]) {
        let mut right = vec![0.0; buf.len()];
        self.process_stereo(buf, &mut right);
        for (l, r) in buf.iter_mut().zip(&right) {
            *l = 0.5 * (*l + r);
        }
    }

    fn process_chunk(&mut self, buf: &mut [f32], right: &mut [f32]) {
        for x in buf.iter_mut() {
            *x *= self.input_gain.tick();
        }
        stage(&mut self.hum_fade, &mut self.dry, buf, |b| self.hum.process(b));
        self.floor.push(buf);
        if let Some(target) = self.floor.floor_db().map(noise_gate::auto_threshold_db) {
            // desce na hora (ruído caiu), sobe no máximo 3 dB/s: nada muda de repente no meio de uma nota
            let max_rise = 3.0 * buf.len() as f32 / self.sample_rate;
            self.auto_threshold_db = Some(match self.auto_threshold_db {
                Some(cur) if target > cur => (cur + max_rise).min(target),
                _ => target,
            });
        }
        if self.auto_gate {
            self.gate.set_threshold_db(self.auto_threshold_db.unwrap_or(self.manual_threshold_db));
        }
        let dry = &mut self.dry;
        stage(&mut self.gate_fade, dry, buf, |b| self.gate.process(b));
        stage(&mut self.comp_fade, dry, buf, |b| self.comp.process(b));
        stage(&mut self.wah_fade, dry, buf, |b| self.wah.process(b));
        stage(&mut self.octave_fade, dry, buf, |b| self.octave.process(b));
        stage(&mut self.boost_fade, dry, buf, |b| self.boost.process(b));
        let (drive, drive_os) = (&mut self.drive, &mut self.drive_os);
        stage(&mut self.drive_fade, dry, buf, |b| drive_os.process(b, |hi| drive.process(hi)));
        stage(&mut self.chorus_fade, dry, buf, |b| self.chorus.process(b));
        stage(&mut self.phaser_fade, dry, buf, |b| self.phaser.process(b));
        stage(&mut self.flanger_fade, dry, buf, |b| self.flanger.process(b));
        stage(&mut self.tremolo_fade, dry, buf, |b| self.tremolo.process(b));
        let (amp, amp_os) = (&mut self.amp, &mut self.amp_os);
        stage(&mut self.amp_fade, dry, buf, |b| amp_os.process(b, |hi| amp.process(hi)));
        stage(&mut self.cab_fade, dry, buf, |b| self.cabinet.process(b));
        stage(&mut self.eq_fade, dry, buf, |b| self.eq.process(b));

        // troca brusca: abaixa aqui, antes do delay e do reverb, para o rabo deles continuar soando
        let ducking = self.pending.is_some() || self.pending_ir.is_some();
        if ducking || self.duck_gain < 1.0 {
            let step = if ducking { -self.duck_step } else { self.duck_step };
            for x in buf.iter_mut() {
                self.duck_gain = (self.duck_gain + step).clamp(0.0, 1.0);
                *x *= self.duck_gain;
            }
        }

        stage(&mut self.delay_fade, dry, buf, |b| self.delay.process(b));
        // daqui em diante, os dois lados
        right.copy_from_slice(buf);
        let dry_r = &mut self.dry_r;
        stage2(&mut self.spread_fade, (dry, dry_r), buf, right, |l, r| self.spread.process(l, r));
        stage2(&mut self.reverb_fade, (dry, dry_r), buf, right, |l, r| self.reverb.process(l, r));
        for (l, r) in buf.iter_mut().zip(right.iter_mut()) {
            let g = self.master.tick();
            *l = soft_limit(*l * g);
            *r = soft_limit(*r * g);
        }

        if self.duck_gain == 0.0 {
            if let Some(s) = self.pending.take() {
                self.apply_now(&s);
            }
            if self.old_ir.is_none()
                && let Some(conv) = self.pending_ir.take()
            {
                self.old_ir = self.cabinet.swap_convolver(conv);
            }
        }
    }
}

/// Linear até 0,7 e encosta suavemente em 1,0: nunca clipa duro na placa de som.
fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.7;
    let a = x.abs();
    if a <= KNEE { x } else { x.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AmpChannel, DriveKind};

    /// Nota de guitarra sintética: Lá da 5ª corda com harmônicos, ataque e decaimento.
    fn pluck(sr: f32, seconds: f32) -> Vec<f32> {
        (0..(sr * seconds) as usize)
            .map(|i| {
                let t = i as f32 / sr;
                let env = (-t * 2.5).exp();
                (1..8).map(|h| 0.25 / h as f32 * (2.0 * std::f32::consts::PI * 110.0 * h as f32 * t).sin()).sum::<f32>() * env
            })
            .collect()
    }

    /// Correlação normalizada máxima entre `a` atrasado de 0 a 120 amostras e `b`.
    fn best_correlation(a: &[f32], b: &[f32]) -> f32 {
        (0..120)
            .map(|lag| {
                let (x, y) = (&a[lag..], &b[..b.len() - lag]);
                let dot: f32 = x.iter().zip(y).map(|(p, q)| p * q).sum();
                let nx: f32 = x.iter().map(|v| v * v).sum::<f32>().sqrt();
                let ny: f32 = y.iter().map(|v| v * v).sum::<f32>().sqrt();
                dot / (nx * ny + 1e-12)
            })
            .fold(f32::MIN, f32::max)
    }

    /// Caso real de 31/10: com a sobreamostragem ligada o som ficava incompreensível no cabo.
    #[test]
    fn oversampling_keeps_the_same_sound() {
        let sr = 48000.0;
        for channel in AmpChannel::ALL {
            let mut s = Settings::default();
            s.amp.channel = channel;
            s.drive.on = channel != AmpChannel::Clean;
            s.reverb.on = false;
            s.gate.on = false;
            let render = |factor: usize| {
                let mut chain = SignalChain::new(sr, &s, factor);
                let mut buf = pluck(sr, 1.0);
                for block in buf.chunks_mut(144) {
                    chain.process(block);
                }
                buf
            };
            let (plain, over) = (render(1), render(4));
            let c = best_correlation(&over[4800..], &plain[4800..]);
            println!("{channel:?}: correlação {c:.4}");
            assert!(c > 0.9, "{channel:?}: correlação {c:.3} entre 1x e 4x");
        }
    }

    /// Custo da cadeia com tudo ligado, em fração de tempo real (`cargo test --release -- --ignored --nocapture custo`).
    #[test]
    #[ignore]
    fn custo_da_cadeia_inteira() {
        let sr = 48000.0;
        let mut s = Settings::default();
        s.comp.on = true;
        s.wah.on = true;
        s.octave.on = true;
        s.boost.on = true;
        s.drive.on = true;
        s.chorus.on = true;
        s.phaser.on = true;
        s.flanger.on = true;
        s.tremolo.on = true;
        s.amp.channel = AmpChannel::Lead;
        s.eq.on = true;
        s.delay.on = true;
        for factor in [1, 2, 4] {
            let mut chain = SignalChain::new(sr, &s, factor);
            let mut buf = vec![0.0f32; 144];
            let seconds = 20.0;
            let start = std::time::Instant::now();
            for b in 0..(seconds * sr / 144.0) as usize {
                for (i, v) in buf.iter_mut().enumerate() {
                    *v = 0.3 * (2.0 * std::f32::consts::PI * 110.0 * (b * 144 + i) as f32 / sr).sin();
                }
                chain.process(&mut buf);
            }
            let used = start.elapsed().as_secs_f32() / seconds;
            println!("{factor}x: {:.2}% de um núcleo", used * 100.0);
        }
    }

    /// Silêncio depois de uma nota (gate fechado, rabo do reverb e do delay morrendo) não pode custar mais que
    /// tocar: `cargo test --release -- --ignored --nocapture silencio`.
    #[test]
    #[ignore]
    fn custo_do_silencio_depois_da_nota() {
        if std::env::var_os("FTZ").is_some() {
            flush_denormals();
        }
        let sr = 48000.0;
        let mut s = Settings::default();
        s.amp.channel = AmpChannel::Crunch;
        s.eq.on = true;
        s.delay.on = true;
        s.reverb.size = 1.0;
        let mut chain = SignalChain::new(sr, &s, 4);
        let note = pluck(sr, 2.0);
        let mut buf = vec![0.0f32; 144];
        let time = |chain: &mut SignalChain, buf: &mut Vec<f32>, src: &dyn Fn(usize) -> f32, blocks: usize| {
            let mut worst = 0.0f32;
            let start = std::time::Instant::now();
            for b in 0..blocks {
                for (i, v) in buf.iter_mut().enumerate() {
                    *v = src(b * 144 + i);
                }
                let t = std::time::Instant::now();
                chain.process(buf);
                worst = worst.max(t.elapsed().as_secs_f32());
            }
            (start.elapsed().as_secs_f32() / blocks as f32, worst)
        };
        let blocks = (2.0 * sr / 144.0) as usize;
        let (avg, worst) = time(&mut chain, &mut buf, &|i| note.get(i).copied().unwrap_or(0.0), blocks);
        println!("tocando: média {:.1} µs por bloco, pior {:.1} µs", avg * 1e6, worst * 1e6);
        for second in 0..20 {
            let (avg, worst) = time(&mut chain, &mut buf, &|_| 0.0, (sr / 144.0) as usize);
            println!("silêncio, {:2}º s: média {:.1} µs por bloco, pior {:.1} µs", second + 1, avg * 1e6, worst * 1e6);
        }
    }

    #[test]
    fn everything_on_stays_finite_and_bounded() {
        let sr = 48000.0;
        for channel in AmpChannel::ALL {
            for kind in DriveKind::ALL {
                let mut s = Settings { master: 1.0, input_gain_db: 24.0, ..Default::default() };
                s.comp.on = true;
                s.drive.on = true;
                s.drive.kind = kind;
                s.drive.gain = 1.0;
                s.chorus.on = true;
                s.wah = crate::settings::WahCfg { on: true, sens: 1.0, resonance: 1.0, range: 1.0 };
                s.octave = crate::settings::OctaveCfg { on: true, sub: 1.0, up: 1.0, dry: 1.0 };
                s.boost = crate::settings::BoostCfg { on: true, level_db: 20.0, bright: 1.0 };
                s.phaser = crate::settings::PhaserCfg { on: true, rate: 1.0, depth: 1.0, feedback: 1.0 };
                s.flanger = crate::settings::FlangerCfg { on: true, rate: 1.0, depth: 1.0, feedback: 1.0 };
                s.tremolo = crate::settings::TremoloCfg { on: true, rate: 1.0, depth: 1.0, shape: 1.0 };
                s.gate.release_ms = 10.0;
                s.gate.hold_ms = 0.0;
                s.gate.range_db = 10.0;
                s.eq.low_cut_hz = 400.0;
                s.eq.high_cut_hz = 2000.0;
                s.amp.channel = channel;
                s.amp.gain = 1.0;
                s.amp.bass = 1.0;
                s.amp.treble = 1.0;
                s.amp.volume = 1.0;
                s.eq.on = true;
                s.eq.bands_db = [12.0; 5];
                s.delay.on = true;
                s.delay.feedback = 1.0;
                s.reverb.on = true;
                s.reverb.size = 1.0;
                s.reverb.mix = 1.0;
                let mut chain = SignalChain::new(sr, &s, 4);
                let mut x = 1u32;
                for _ in 0..200 {
                    let mut buf: Vec<f32> = (0..480)
                        .map(|_| {
                            x ^= x << 13;
                            x ^= x >> 17;
                            x ^= x << 5;
                            (x as f32 / u32::MAX as f32) - 0.5
                        })
                        .collect();
                    chain.process(&mut buf);
                    assert!(buf.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
                }
            }
        }
    }

    #[test]
    fn auto_gate_keeps_the_note_and_closes_in_silence() {
        let sr = 48000.0;
        let mut s = Settings::default();
        s.comp.on = false;
        s.amp.on = false;
        s.cab.on = false;
        s.reverb.on = false;
        let mut chain = SignalChain::new(sr, &s, 4);
        let mut seed = 7u32;
        let mut hiss = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            ((seed as f32 / u32::MAX as f32) - 0.5) * db_to_lin(-84.0) * 3.46
        };
        let hum = |i: usize| db_to_lin(-62.0) * 1.414 * (2.0 * std::f32::consts::PI * 60.0 * i as f32 / sr).sin();
        let mut n = 0usize;
        let mut run = |seconds: f32, note: &dyn Fn(f32) -> f32, check: &mut dyn FnMut(f32, bool)| {
            let blocks = (seconds * sr / 480.0) as usize;
            for _ in 0..blocks {
                let start_t = n as f32 / sr;
                let mut buf: Vec<f32> = (0..480).map(|k| hum(n + k) + hiss() + note((n + k) as f32 / sr)).collect();
                n += 480;
                chain.process(&mut buf);
                check(start_t, chain.gate_open());
            }
        };
        // ruído do cabo parado
        run(3.0, &|_| 0.0, &mut |_, _| {});
        let mut closed = false;
        run(0.5, &|_| 0.0, &mut |_, open| closed = !open);
        assert!(closed, "o gate tem que fechar no silêncio");
        // uma nota de Sol3 que morre devagar: de -12 dB até sumir em uns 6 s
        let t0 = 3.5f32; // 3 s + 0,5 s em blocos inteiros de 480
        let note = move |t: f32| db_to_lin(-12.0) * 1.414 * (-(t - t0) / 0.9).exp() * (2.0 * std::f32::consts::PI * 196.0 * t).sin();
        run(6.0, &note, &mut |t, open| {
            let level = -12.0 - 8.686 * (t - t0) / 0.9;
            if level > -60.0 {
                assert!(open, "cortou a nota aos {:.2} s, nível {level:.0} dB", t - t0);
            }
        });
        // silêncio de novo
        let mut closed = false;
        run(2.0, &|_| 0.0, &mut |_, open| closed = !open);
        assert!(closed, "o gate tem que fechar depois da nota");
    }

    /// Maior salto entre duas amostras seguidas no trecho, em relação ao maior salto do som parado.
    fn worst_jump(buf: &[f32]) -> f32 {
        buf.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    /// Pisar num pedal, trocar o canal do amp ou o preset no meio de uma nota não pode estalar.
    #[test]
    fn switching_in_the_middle_of_a_note_does_not_click() {
        let sr = 48000.0;
        let tone = |i: usize| 0.25 * (2.0 * std::f32::consts::PI * 110.0 * i as f32 / sr).sin();
        let mut base = Settings::default();
        base.gate.on = false;
        base.reverb.on = false;
        base.amp.on = false;
        base.cab.on = false;
        type Edit = fn(&mut Settings);
        let cases: [(&str, Edit); 9] = [
            ("liga o drive", |s| s.drive.on = true),
            ("desliga o reverb", |s| s.reverb.on = false),
            ("liga o amp", |s| s.amp.on = true),
            ("liga o boost", |s| s.boost.on = true),
            ("liga o tremolo", |s| s.tremolo.on = true),
            ("liga o delay", |s| s.delay.on = true),
            ("liga o gabinete", |s| s.cab.on = true),
            ("troca o canal do amp", |s| {
                s.amp.on = true;
                s.amp.channel = AmpChannel::Lead;
            }),
            ("troca de preset", |s| {
                s.amp.on = true;
                s.amp.channel = AmpChannel::Lead;
                s.amp.gain = 0.9;
                s.cab.on = true;
                s.drive.on = true;
                s.drive.gain = 0.8;
            }),
        ];
        for (what, edit) in cases {
            let mut before = base;
            match what {
                "troca o canal do amp" => before.amp.on = true,
                "desliga o reverb" => before.reverb.on = true,
                _ => {}
            }
            let mut after = before;
            edit(&mut after);
            let mut chain = SignalChain::new(sr, &before, 1);
            let mut n = 0;
            let mut run = |chain: &mut SignalChain, blocks: usize| {
                let mut out = Vec::new();
                for _ in 0..blocks {
                    let mut buf: Vec<f32> = (n..n + 144).map(tone).collect();
                    n += 144;
                    chain.process(&mut buf);
                    out.extend_from_slice(&buf);
                }
                out
            };
            let steady_before = worst_jump(&run(&mut chain, 100)[2000..]);
            chain.apply(&after, what == "troca de preset");
            let switch = run(&mut chain, 20);
            let steady_after = worst_jump(&run(&mut chain, 100)[2000..]);
            let jump = worst_jump(&switch);
            let normal = steady_before.max(steady_after);
            println!("{what}: salto {:.2}x o normal", jump / normal);
            assert!(jump < normal * 1.5, "{what}: salto de {jump:.4} na troca, o normal é {normal:.4}");
        }
    }

    #[test]
    fn mute_silences_the_output() {
        let s = Settings { mute: true, ..Default::default() };
        let mut chain = SignalChain::new(48000.0, &s, 2);
        let mut buf = vec![0.5f32; 4800];
        chain.process(&mut buf);
        assert!(buf[2400..].iter().all(|v| v.abs() < 1e-3));
    }
}
