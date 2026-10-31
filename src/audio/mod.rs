pub mod diagnostic;
pub mod recorder;
#[cfg(windows)]
pub mod wasapi_io;

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use crossbeam_channel::{Receiver, Sender};

use crate::dsp::SignalChain;
use crate::dsp::cabinet::Convolver;
use crate::meters::Meters;
use crate::preset::{Exclusive, LatencyMode, Oversample};
use crate::settings::Settings;

pub struct Devices {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

pub fn list_devices() -> Devices {
    let host = cpal::default_host();
    let names = |it: Option<Vec<cpal::Device>>| -> Vec<String> {
        it.unwrap_or_default().iter().filter_map(|d| d.name().ok()).collect()
    };
    Devices {
        inputs: names(host.input_devices().ok().map(|d| d.collect())),
        outputs: names(host.output_devices().ok().map(|d| d.collect())),
    }
}

/// `--teste-motor`: liga o motor mudo (nada chega ao fone) e mede blocos, fila, atraso e falhas.
pub fn engine_test(seconds: u64, exclusive: Exclusive, latency: LatencyMode, output: Option<&str>) -> Result<String> {
    let meters = Arc::new(Meters::new());
    let settings = Settings { mute: true, ..Settings::default() };
    let engine = Engine::start(None, output, latency, exclusive, Oversample::default(), &settings, meters.clone())?;
    let mut out = format!(
        "{} ({} Hz) -> {} ({} Hz), exclusivo: entrada {} saída {}\n",
        engine.info.input, engine.info.rate, engine.info.output, engine.info.out_rate, engine.info.exclusive.input, engine.info.exclusive.output
    );
    if let Some(n) = &engine.info.notice {
        let _ = writeln!(out, "aviso: {n}");
    }
    // a partida descarta a sobra do anel uma vez e o anel acha a folga dele: mede só o regime
    std::thread::sleep(std::time::Duration::from_secs(1));
    meters.underruns.store(0, Relaxed);
    meters.ring_drops.store(0, Relaxed);
    let (t0, f0) = (std::time::Instant::now(), meters.out_frames.load(Relaxed));
    let mut lat = Vec::new();
    for _ in 0..seconds * 10 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        lat.push(engine.latency_ms(&meters));
    }
    lat.sort_by(f32::total_cmp);
    if engine.info.exclusive.output {
        let rate = (meters.out_frames.load(Relaxed) - f0) as f64 / t0.elapsed().as_secs_f64();
        let _ = writeln!(out, "a saída pediu {rate:.0} amostras/s (devia ser {})", engine.info.out_rate);
        let fallback = meters.out_fallback.load(Relaxed);
        if fallback > 0 {
            let _ = writeln!(out, "a saída exclusiva pediu {fallback} amostras/s e passou para o compartilhado");
        }
    }
    let _ = writeln!(
        out,
        "bloco de entrada {} amostras, de saída {}, fila exclusiva {}, anel agora {}",
        meters.in_block.load(Relaxed),
        meters.out_block.load(Relaxed),
        meters.out_queue.load(Relaxed),
        meters.ring_fill.load(Relaxed)
    );
    let _ = writeln!(
        out,
        "atraso do app: mínimo {:.1} ms, mediana {:.1} ms, máximo {:.1} ms; falhas {}; descartadas {}; erro de dispositivo {}",
        lat[0],
        lat[lat.len() / 2],
        lat[lat.len() - 1],
        meters.underruns.load(Relaxed),
        meters.ring_drops.load(Relaxed),
        meters.device_error.load(Relaxed)
    );
    Ok(out)
}

/// Texto com os dispositivos e o formato padrão de cada um (para `--dispositivos`).
pub fn describe() -> String {
    let host = cpal::default_host();
    let mut out = String::new();
    let _ = writeln!(out, "Host de áudio: {:?}", host.id());
    if let Ok(devs) = host.input_devices() {
        let _ = writeln!(out, "\nEntradas:");
        for d in devs {
            let cfg = d.default_input_config().map(|c| format!("{} ch, {} Hz, {:?}", c.channels(), c.sample_rate().0, c.sample_format()));
            let _ = writeln!(out, "  {} -> {}", d.name().unwrap_or_default(), cfg.unwrap_or_else(|e| e.to_string()));
        }
    }
    if let Ok(devs) = host.output_devices() {
        let _ = writeln!(out, "\nSaídas:");
        for d in devs {
            let cfg = d.default_output_config().map(|c| format!("{} ch, {} Hz, {:?}", c.channels(), c.sample_rate().0, c.sample_format()));
            let _ = writeln!(out, "  {} -> {}", d.name().unwrap_or_default(), cfg.unwrap_or_else(|e| e.to_string()));
        }
    }
    out
}

pub fn is_rocksmith(name: &str) -> bool {
    name.to_lowercase().contains("rocksmith")
}

fn find_device(devices: impl Iterator<Item = cpal::Device>, wanted: impl Fn(&str) -> bool) -> Option<cpal::Device> {
    devices.into_iter().find(|d| d.name().map(|n| wanted(&n)).unwrap_or(false))
}

fn pick_input(host: &cpal::Host, preferred: Option<&str>) -> Result<cpal::Device> {
    if let Some(p) = preferred {
        if let Some(d) = find_device(host.input_devices()?, |n| n == p) {
            return Ok(d);
        }
        log::warn!("entrada '{p}' não encontrada, procurando o cabo Rocksmith");
    }
    if let Some(d) = find_device(host.input_devices()?, is_rocksmith) {
        return Ok(d);
    }
    host.default_input_device().context("nenhum dispositivo de entrada de áudio")
}

fn pick_output(host: &cpal::Host, preferred: Option<&str>) -> Result<cpal::Device> {
    if let Some(p) = preferred {
        if let Some(d) = find_device(host.output_devices()?, |n| n == p) {
            return Ok(d);
        }
        log::warn!("saída '{p}' não encontrada, usando a padrão do Windows");
    }
    host.default_output_device().context("nenhum dispositivo de saída de áudio")
}

pub struct EngineInfo {
    pub input: String,
    pub output: String,
    pub rate: u32,
    pub out_rate: u32,
    /// Lados que abriram no modo exclusivo.
    pub exclusive: Exclusive,
    /// Exclusivo pedido e negado (o lado caiu para o compartilhado), com o motivo.
    pub notice: Option<String>,
}

/// Um lado do áudio aberto; soltar fecha.
enum Stream {
    Cpal(#[allow(dead_code)] cpal::Stream),
    #[cfg(windows)]
    Wasapi(#[allow(dead_code)] wasapi_io::Stream),
}

/// Motor de áudio. A cadeia de efeitos vive dentro da callback de entrada; a interface conversa
/// com ela só por canais e atômicos, nunca por trava.
pub struct Engine {
    _input: Stream,
    _output: Stream,
    pub info: EngineInfo,
    settings_tx: Sender<Settings>,
    ir_tx: Sender<Option<Box<Convolver>>>,
    trash_rx: Receiver<Box<Convolver>>,
    rec_rx: Option<rtrb::Consumer<f32>>,
    tap_rx: Option<rtrb::Consumer<f32>>,
}

impl Engine {
    pub fn start(
        input_pref: Option<&str>,
        output_pref: Option<&str>,
        latency: LatencyMode,
        exclusive: Exclusive,
        oversample: Oversample,
        settings: &Settings,
        meters: Arc<Meters>,
    ) -> Result<Self> {
        let host = cpal::default_host();
        let in_dev = pick_input(&host, input_pref)?;
        let out_dev = pick_output(&host, output_pref)?;
        let input_name = in_dev.name().unwrap_or_else(|_| "entrada".into());
        let output_name = out_dev.name().unwrap_or_else(|_| "saída".into());

        meters.reset_engine_counters();
        let mut notices = Vec::new();

        // Exclusivo primeiro; se o Windows negar, o lado cai para o compartilhado do cpal.
        #[cfg(windows)]
        let ex_in = exclusive
            .input
            .then(|| wasapi_io::open_capture(&input_name, latency.exclusive_period_ms(), meters.clone()))
            .and_then(|r| r.map_err(|e| notices.push(format!("entrada exclusiva: {e}"))).ok());
        #[cfg(windows)]
        let in_format = match &ex_in {
            Some(p) => (p.info.rate, p.info.channels, format!("{} exclusivo", p.info.format)),
            None => shared_format(in_dev.default_input_config(), &input_name)?,
        };
        #[cfg(not(windows))]
        let in_format = shared_format(in_dev.default_input_config(), &input_name)?;
        let (rate, in_channels, in_desc) = in_format;

        #[cfg(windows)]
        let ex_out = exclusive
            .output
            .then(|| wasapi_io::open_render(&output_name, rate, latency.exclusive_period_ms(), meters.clone()))
            .and_then(|r| r.map_err(|e| notices.push(format!("saída exclusiva: {e}"))).ok());
        #[cfg(windows)]
        let out_format = match &ex_out {
            Some(p) => (p.info.rate, p.info.channels, format!("{} exclusivo", p.info.format)),
            None => shared_format(out_dev.default_output_config(), &output_name)?,
        };
        #[cfg(not(windows))]
        let out_format = shared_format(out_dev.default_output_config(), &output_name)?;
        let (out_rate, out_channels, out_desc) = out_format;
        log::info!("entrada: {input_name} ({in_desc}) | saída: {output_name} ({out_desc})");
        for n in &notices {
            log::warn!("{n}; usando o modo compartilhado");
        }

        let (settings_tx, settings_rx) = crossbeam_channel::bounded(64);
        let (ir_tx, ir_rx) = crossbeam_channel::bounded(4);
        let (trash_tx, trash_rx) = crossbeam_channel::bounded(4);
        let (out_prod, out_cons) = rtrb::RingBuffer::new(rate as usize / 2);
        let (an_prod, an_cons) = rtrb::RingBuffer::new(rate as usize / 2);
        let (rec_prod, rec_cons) = rtrb::RingBuffer::new(rate as usize * 4);
        let (tap_prod, tap_cons) = rtrb::RingBuffer::new(out_rate as usize / 2);

        crate::analysis::spawn(an_cons, rate as f32, meters.clone());

        let input = InputSide {
            chain: SignalChain::new(rate as f32, settings, oversample.factor()),
            settings_rx,
            ir_rx,
            trash_tx,
            out: out_prod,
            analysis: an_prod,
            rec: rec_prod,
            meters: meters.clone(),
            mono: Vec::with_capacity(16384),
            dry: Vec::with_capacity(16384),
            channels: in_channels,
            last: None,
        };
        let ratio = rate as f64 / out_rate as f64;
        let output = OutputSide {
            ring: out_cons,
            nominal: ratio,
            ratio,
            frac: 1.0,
            a: 0.0,
            b: 0.0,
            priming: true,
            fill_avg: 0.0,
            margin: latency.margin_blocks(),
            channels: out_channels,
            meters: meters.clone(),
            tap: tap_prod,
            // o mixer do Windows (saída compartilhada) chama em horários bem menos certos que o exclusivo
            jitter: if out_desc.ends_with("exclusivo") { 0.0 } else { rate as f32 * 0.002 },
            jitter_max: rate as f32 * 0.02,
        };

        let busy_hint = "Se o Rocksmith ou outro programa de áudio estiver aberto, feche e tente de novo.";
        let open_in = |side| open_input(&in_dev, side, &meters).with_context(|| format!("abrindo '{input_name}'. {busy_hint}"));
        let open_out = |side| open_output(&out_dev, side, &meters).with_context(|| format!("abrindo '{output_name}'"));
        #[cfg(windows)]
        let input_stream = match ex_in {
            Some(p) => {
                let mut side = input;
                Stream::Wasapi(p.start(Box::new(move |data: &[f32]| side.callback(data))))
            }
            None => Stream::Cpal(open_in(input)?),
        };
        #[cfg(not(windows))]
        let input_stream = Stream::Cpal(open_in(input)?);
        #[cfg(windows)]
        let output_stream = match ex_out {
            Some(p) => {
                let mut side = output;
                Stream::Wasapi(p.start(Box::new(move |data: &mut [f32]| side.callback(data))))
            }
            None => Stream::Cpal(open_out(output)?),
        };
        #[cfg(not(windows))]
        let output_stream = Stream::Cpal(open_out(output)?);
        #[cfg(windows)]
        let opened = Exclusive {
            input: matches!(input_stream, Stream::Wasapi(_)),
            output: matches!(output_stream, Stream::Wasapi(_)),
        };
        #[cfg(not(windows))]
        let opened = Exclusive { input: false, output: false };
        let notice = (!notices.is_empty()).then(|| notices.join("; "));

        Ok(Self {
            _input: input_stream,
            _output: output_stream,
            info: EngineInfo { input: input_name, output: output_name, rate, out_rate, exclusive: opened, notice },
            settings_tx,
            ir_tx,
            trash_rx,
            rec_rx: Some(rec_cons),
            tap_rx: Some(tap_cons),
        })
    }

    pub fn send_settings(&self, s: Settings) -> bool {
        self.settings_tx.try_send(s).is_ok()
    }

    pub fn send_ir(&self, conv: Option<Box<Convolver>>) {
        let _ = self.ir_tx.try_send(conv);
    }

    /// Convolvers trocados voltam da thread de áudio para serem liberados aqui.
    pub fn collect_trash(&self) {
        while self.trash_rx.try_recv().is_ok() {}
    }

    pub fn take_recorder_input(&mut self) -> Option<rtrb::Consumer<f32>> {
        self.rec_rx.take()
    }

    /// O que a saída mandou ao dispositivo (1º canal, com os silêncios), para o diagnóstico.
    pub fn take_output_tap(&mut self) -> Option<rtrb::Consumer<f32>> {
        self.tap_rx.take()
    }

    pub fn return_recorder_input(&mut self, rx: rtrb::Consumer<f32>) {
        self.rec_rx = Some(rx);
    }

    /// Atraso do próprio app (blocos + anel). O Windows soma o dele por cima.
    pub fn latency_ms(&self, m: &Meters) -> f32 {
        let samples = m.in_block.load(Relaxed) + m.ring_fill.load(Relaxed);
        let out = m.out_block.load(Relaxed).max(m.out_queue.load(Relaxed)) as f32 / self.info.out_rate as f32;
        (samples as f32 / self.info.rate as f32 + out) * 1000.0
    }
}

/// Formato do compartilhado (o mixer do Windows): taxa, canais e a descrição para o log.
fn shared_format(
    cfg: std::result::Result<cpal::SupportedStreamConfig, cpal::DefaultStreamConfigError>,
    name: &str,
) -> Result<(u32, usize, String)> {
    let cfg = cfg.with_context(|| format!("formato de '{name}'"))?;
    let desc = format!("{} ch, {} Hz, {:?} compartilhado", cfg.channels(), cfg.sample_rate().0, cfg.sample_format());
    Ok((cfg.sample_rate().0, cfg.channels() as usize, desc))
}

fn stream_config(cfg: &cpal::SupportedStreamConfig) -> cpal::StreamConfig {
    cpal::StreamConfig {
        channels: cfg.channels(),
        sample_rate: cfg.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    }
}

fn open_input(dev: &cpal::Device, side: InputSide, meters: &Arc<Meters>) -> Result<cpal::Stream> {
    let cfg = dev.default_input_config()?;
    let stream_cfg = stream_config(&cfg);
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => build_input::<f32>(dev, &stream_cfg, side, meters),
        SampleFormat::I16 => build_input::<i16>(dev, &stream_cfg, side, meters),
        SampleFormat::I32 => build_input::<i32>(dev, &stream_cfg, side, meters),
        SampleFormat::U16 => build_input::<u16>(dev, &stream_cfg, side, meters),
        f => Err(anyhow!("formato de entrada não suportado: {f:?}")),
    }?;
    stream.play()?;
    Ok(stream)
}

fn open_output(dev: &cpal::Device, side: OutputSide, meters: &Arc<Meters>) -> Result<cpal::Stream> {
    let cfg = dev.default_output_config()?;
    let stream_cfg = stream_config(&cfg);
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => build_output::<f32>(dev, &stream_cfg, side, meters),
        SampleFormat::I16 => build_output::<i16>(dev, &stream_cfg, side, meters),
        SampleFormat::I32 => build_output::<i32>(dev, &stream_cfg, side, meters),
        SampleFormat::U16 => build_output::<u16>(dev, &stream_cfg, side, meters),
        f => Err(anyhow!("formato de saída não suportado: {f:?}")),
    }?;
    stream.play()?;
    Ok(stream)
}

fn build_input<T>(dev: &cpal::Device, cfg: &cpal::StreamConfig, mut side: InputSide, meters: &Arc<Meters>) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let m = meters.clone();
    Ok(dev.build_input_stream(
        cfg,
        move |data: &[T], _: &cpal::InputCallbackInfo| side.callback(data),
        move |e| {
            log::error!("erro na entrada: {e}");
            m.device_error.store(true, Relaxed);
        },
        None,
    )?)
}

fn build_output<T>(dev: &cpal::Device, cfg: &cpal::StreamConfig, mut side: OutputSide, meters: &Arc<Meters>) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let m = meters.clone();
    Ok(dev.build_output_stream(
        cfg,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| side.callback(data),
        move |e| {
            log::error!("erro na saída: {e}");
            m.device_error.store(true, Relaxed);
        },
        None,
    )?)
}

struct InputSide {
    chain: SignalChain,
    settings_rx: Receiver<Settings>,
    ir_rx: Receiver<Option<Box<Convolver>>>,
    trash_tx: Sender<Box<Convolver>>,
    out: rtrb::Producer<f32>,
    analysis: rtrb::Producer<f32>,
    rec: rtrb::Producer<f32>,
    meters: Arc<Meters>,
    mono: Vec<f32>,
    dry: Vec<f32>,
    channels: usize,
    last: Option<std::time::Instant>,
}

impl InputSide {
    fn callback<T>(&mut self, data: &[T])
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let started = std::time::Instant::now();
        if let Some(prev) = self.last.replace(started) {
            self.meters.in_gap_max_us.fetch_max(started.duration_since(prev).as_micros() as u32, Relaxed);
        }
        let mut latest = None;
        while let Ok(s) = self.settings_rx.try_recv() {
            latest = Some(s);
        }
        if let Some(s) = latest {
            self.chain.apply(&s);
        }
        while let Ok(conv) = self.ir_rx.try_recv() {
            if let Some(old) = self.chain.cabinet.swap_convolver(conv) {
                let _ = self.trash_tx.try_send(old);
            }
        }

        self.mono.clear();
        let mut peak = 0.0f32;
        for frame in data.chunks(self.channels) {
            let v: f32 = frame[0].to_sample();
            peak = peak.max(v.abs());
            self.mono.push(v);
        }
        self.meters.in_peak.raise(peak);
        self.meters.in_block.store(self.mono.len() as u32, Relaxed);

        for &v in &self.mono {
            if self.analysis.push(v).is_err() {
                break;
            }
        }

        let recording = self.meters.recording.load(Relaxed);
        if recording {
            self.dry.clear();
            self.dry.extend_from_slice(&self.mono);
        }

        self.chain.set_mains(self.meters.mains_hz.get());
        self.chain.process(&mut self.mono);
        self.meters.gate_auto_db.set(self.chain.auto_threshold_db.unwrap_or(-120.0));
        self.meters.gate_open.store(self.chain.gate_open(), Relaxed);

        for &v in &self.mono {
            if self.out.push(v).is_err() {
                break;
            }
        }

        if recording {
            let mut dropped = 0;
            for (&d, &w) in self.dry.iter().zip(&self.mono) {
                if self.rec.slots() < 2 {
                    dropped += 1;
                    continue;
                }
                let _ = self.rec.push(d);
                let _ = self.rec.push(w);
            }
            if dropped > 0 {
                self.meters.rec_dropped.fetch_add(dropped, Relaxed);
            }
        }
        self.meters.proc_max_us.fetch_max(started.elapsed().as_micros() as u32, Relaxed);
    }
}

/// Lado da saída: lê o anel com interpolação linear. A razão de leitura acompanha o nível
/// do anel para compensar a diferença de relógio entre o cabo e a placa de som.
struct OutputSide {
    ring: rtrb::Consumer<f32>,
    nominal: f64,
    ratio: f64,
    frac: f64,
    a: f32,
    b: f32,
    priming: bool,
    fill_avg: f32,
    margin: f32,
    channels: usize,
    meters: Arc<Meters>,
    tap: rtrb::Producer<f32>,
    /// Reserva pelo tremor de horário entre as duas threads (amostras da entrada). Começa pequena e cresce um
    /// bloco de entrada a cada falha, até 20 ms: cada máquina acha a sua sem o usuário escolher.
    jitter: f32,
    jitter_max: f32,
}

impl OutputSide {
    fn callback<T>(&mut self, data: &mut [T])
    where
        T: SizedSample + FromSample<f32>,
    {
        let silence = T::from_sample(0.0f32);
        let frames = data.len() / self.channels.max(1);
        self.meters.out_block.store(frames as u32, Relaxed);
        let in_block = self.meters.in_block.load(Relaxed).max(64) as f32;
        let need = (frames as f64 * self.ratio).ceil() as f32;
        if self.jitter < self.jitter_max * 0.1 && self.meters.out_fallback.load(Relaxed) > 0 {
            // a saída exclusiva caiu para o compartilhado: o mixer do Windows treme mais
            self.jitter = self.jitter_max * 0.1;
        }
        let target = need + in_block * self.margin + self.jitter;
        let avail = self.ring.slots();
        self.meters.ring_fill.store(avail as u32, Relaxed);

        if self.priming {
            if (avail as f32) < target {
                data.fill(silence);
                for _ in 0..frames {
                    let _ = self.tap.push(0.0);
                }
                return;
            }
            self.priming = false;
            self.fill_avg = avail as f32;
        }

        // o nível oscila um bloco de saída inteiro (ela leva tudo de uma vez) mais o de entrada; só descarta se
        // passar disso, senão joga fora som bom a cada tremida (31/10: 2270 amostras em 15 s com entrada exclusiva)
        let max_fill = (target + 2.0 * in_block.max(need)) as usize;
        if avail > max_fill {
            let excess = avail - target as usize;
            if let Ok(chunk) = self.ring.read_chunk(excess) {
                chunk.commit_all();
                self.meters.ring_drops.fetch_add(excess as u32, Relaxed);
            }
        }

        self.fill_avg += 0.02 * (self.ring.slots() as f32 - self.fill_avg);
        let err = ((self.fill_avg - target) / target).clamp(-1.0, 1.0) as f64;
        self.ratio = self.nominal * (1.0 + 0.003 * err);

        let mut peak = 0.0f32;
        for frame in data.chunks_mut(self.channels.max(1)) {
            if !self.priming {
                while self.frac >= 1.0 {
                    match self.ring.pop() {
                        Ok(v) => {
                            self.a = self.b;
                            self.b = v;
                            self.frac -= 1.0;
                        }
                        Err(_) => {
                            self.priming = true;
                            self.a = 0.0;
                            self.b = 0.0;
                            self.frac = 1.0;
                            self.meters.underruns.fetch_add(1, Relaxed);
                            self.jitter = (self.jitter + in_block).min(self.jitter_max);
                            break;
                        }
                    }
                }
            }
            if self.priming {
                frame.fill(silence);
                let _ = self.tap.push(0.0);
                continue;
            }
            let v = self.a + (self.b - self.a) * self.frac as f32;
            let _ = self.tap.push(v);
            self.frac += self.ratio;
            peak = peak.max(v.abs());
            // só frente esquerda e direita: em saída 5.1/7.1 (headset JBL abre com 8) o resto fica mudo
            let t = T::from_sample(v);
            for (i, out) in frame.iter_mut().enumerate() {
                *out = if i < 2 { t } else { silence };
            }
        }
        self.meters.out_peak.raise(peak);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn output_side(ring: rtrb::Consumer<f32>, in_rate: f64, out_rate: f64, meters: Arc<Meters>) -> OutputSide {
        let ratio = in_rate / out_rate;
        OutputSide {
            ring,
            nominal: ratio,
            ratio,
            frac: 1.0,
            a: 0.0,
            b: 0.0,
            priming: true,
            fill_avg: 0.0,
            margin: 1.0,
            channels: 2,
            meters,
            tap: rtrb::RingBuffer::new(16).0,
            jitter: 0.0,
            jitter_max: 960.0,
        }
    }

    #[test]
    fn resamples_48k_to_44k1_without_underruns() {
        let meters = Arc::new(Meters::new());
        let (mut prod, cons) = rtrb::RingBuffer::new(24000);
        let mut out = output_side(cons, 48000.0, 44100.0, meters.clone());
        meters.in_block.store(480, Relaxed);

        let mut n_in = 0usize;
        let mut collected = Vec::new();
        for step in 0..300 {
            for _ in 0..480 {
                let t = n_in as f32 / 48000.0;
                let _ = prod.push((2.0 * PI * 1000.0 * t).sin() * 0.5);
                n_in += 1;
            }
            let mut buf = vec![0.0f32; 441 * 2];
            out.callback(&mut buf);
            if step > 20 {
                collected.extend(buf.chunks(2).map(|f| f[0]));
            }
        }
        assert_eq!(meters.underruns.load(Relaxed), 0);
        let crossings = collected.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let seconds = collected.len() as f32 / 44100.0;
        let freq = crossings as f32 / seconds;
        assert!((freq - 1000.0).abs() < 5.0, "frequência {freq}");
    }

    #[test]
    fn absorbs_clock_drift_without_growing_latency() {
        let meters = Arc::new(Meters::new());
        let (mut prod, cons) = rtrb::RingBuffer::new(24000);
        let mut out = output_side(cons, 48000.0, 48000.0, meters.clone());
        meters.in_block.store(480, Relaxed);
        // entrada 0,1% mais rápida que a saída: 481 amostras a cada 480 lidas
        for _ in 0..3000 {
            for _ in 0..481 {
                let _ = prod.push(0.1);
            }
            let mut buf = vec![0.0f32; 480 * 2];
            out.callback(&mut buf);
        }
        let fill = meters.ring_fill.load(Relaxed);
        assert!(fill < 480 * 4, "anel cresceu para {fill}");
        assert_eq!(meters.underruns.load(Relaxed), 0);
    }
}
