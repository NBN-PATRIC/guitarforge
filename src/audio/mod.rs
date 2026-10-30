pub mod recorder;

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
use crate::preset::LatencyMode;
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
}

/// Motor de áudio. A cadeia de efeitos vive dentro da callback de entrada; a interface conversa
/// com ela só por canais e atômicos, nunca por trava.
pub struct Engine {
    _input: cpal::Stream,
    _output: cpal::Stream,
    pub info: EngineInfo,
    settings_tx: Sender<Settings>,
    ir_tx: Sender<Option<Box<Convolver>>>,
    trash_rx: Receiver<Box<Convolver>>,
    rec_rx: Option<rtrb::Consumer<f32>>,
}

impl Engine {
    pub fn start(
        input_pref: Option<&str>,
        output_pref: Option<&str>,
        latency: LatencyMode,
        settings: &Settings,
        meters: Arc<Meters>,
    ) -> Result<Self> {
        let host = cpal::default_host();
        let in_dev = pick_input(&host, input_pref)?;
        let out_dev = pick_output(&host, output_pref)?;
        let input_name = in_dev.name().unwrap_or_else(|_| "entrada".into());
        let output_name = out_dev.name().unwrap_or_else(|_| "saída".into());

        let in_cfg = in_dev.default_input_config().with_context(|| format!("formato de '{input_name}'"))?;
        let out_cfg = out_dev.default_output_config().with_context(|| format!("formato de '{output_name}'"))?;
        let rate = in_cfg.sample_rate().0;
        let out_rate = out_cfg.sample_rate().0;
        log::info!(
            "entrada: {input_name} ({} ch, {rate} Hz, {:?}) | saída: {output_name} ({} ch, {out_rate} Hz, {:?})",
            in_cfg.channels(),
            in_cfg.sample_format(),
            out_cfg.channels(),
            out_cfg.sample_format()
        );

        meters.reset_engine_counters();
        let (settings_tx, settings_rx) = crossbeam_channel::bounded(64);
        let (ir_tx, ir_rx) = crossbeam_channel::bounded(4);
        let (trash_tx, trash_rx) = crossbeam_channel::bounded(4);
        let (out_prod, out_cons) = rtrb::RingBuffer::new(rate as usize / 2);
        let (an_prod, an_cons) = rtrb::RingBuffer::new(rate as usize / 2);
        let (rec_prod, rec_cons) = rtrb::RingBuffer::new(rate as usize * 4);

        crate::analysis::spawn(an_cons, rate as f32, meters.clone());

        let input = InputSide {
            chain: SignalChain::new(rate as f32, settings),
            settings_rx,
            ir_rx,
            trash_tx,
            out: out_prod,
            analysis: an_prod,
            rec: rec_prod,
            meters: meters.clone(),
            mono: Vec::with_capacity(16384),
            dry: Vec::with_capacity(16384),
            channels: in_cfg.channels() as usize,
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
            channels: out_cfg.channels() as usize,
            meters: meters.clone(),
        };

        let in_stream_cfg = cpal::StreamConfig {
            channels: in_cfg.channels(),
            sample_rate: in_cfg.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };
        let out_stream_cfg = cpal::StreamConfig {
            channels: out_cfg.channels(),
            sample_rate: out_cfg.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let busy_hint = "Se o Rocksmith ou outro programa de áudio estiver aberto, feche e tente de novo.";
        let input_stream = match in_cfg.sample_format() {
            SampleFormat::F32 => build_input::<f32>(&in_dev, &in_stream_cfg, input, &meters),
            SampleFormat::I16 => build_input::<i16>(&in_dev, &in_stream_cfg, input, &meters),
            SampleFormat::I32 => build_input::<i32>(&in_dev, &in_stream_cfg, input, &meters),
            SampleFormat::U16 => build_input::<u16>(&in_dev, &in_stream_cfg, input, &meters),
            f => Err(anyhow!("formato de entrada não suportado: {f:?}")),
        }
        .with_context(|| format!("abrindo '{input_name}'. {busy_hint}"))?;
        let output_stream = match out_cfg.sample_format() {
            SampleFormat::F32 => build_output::<f32>(&out_dev, &out_stream_cfg, output, &meters),
            SampleFormat::I16 => build_output::<i16>(&out_dev, &out_stream_cfg, output, &meters),
            SampleFormat::I32 => build_output::<i32>(&out_dev, &out_stream_cfg, output, &meters),
            SampleFormat::U16 => build_output::<u16>(&out_dev, &out_stream_cfg, output, &meters),
            f => Err(anyhow!("formato de saída não suportado: {f:?}")),
        }
        .with_context(|| format!("abrindo '{output_name}'"))?;

        input_stream.play()?;
        output_stream.play()?;

        Ok(Self {
            _input: input_stream,
            _output: output_stream,
            info: EngineInfo { input: input_name, output: output_name, rate, out_rate },
            settings_tx,
            ir_tx,
            trash_rx,
            rec_rx: Some(rec_cons),
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

    pub fn return_recorder_input(&mut self, rx: rtrb::Consumer<f32>) {
        self.rec_rx = Some(rx);
    }

    /// Atraso do próprio app (blocos + anel). O Windows soma o dele por cima.
    pub fn latency_ms(&self, m: &Meters) -> f32 {
        let samples = m.in_block.load(Relaxed) + m.ring_fill.load(Relaxed);
        let out = m.out_block.load(Relaxed) as f32 / self.info.out_rate as f32;
        (samples as f32 / self.info.rate as f32 + out) * 1000.0
    }
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
}

impl InputSide {
    fn callback<T>(&mut self, data: &[T])
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
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
        let target = need + in_block * self.margin;
        let avail = self.ring.slots();
        self.meters.ring_fill.store(avail as u32, Relaxed);

        if self.priming {
            if (avail as f32) < target {
                data.fill(silence);
                return;
            }
            self.priming = false;
            self.fill_avg = avail as f32;
        }

        let max_fill = (target + 2.0 * in_block) as usize;
        if avail > max_fill {
            let excess = avail - target as usize;
            if let Ok(chunk) = self.ring.read_chunk(excess) {
                chunk.commit_all();
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
                            break;
                        }
                    }
                }
            }
            if self.priming {
                frame.fill(silence);
                continue;
            }
            let v = self.a + (self.b - self.a) * self.frac as f32;
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
