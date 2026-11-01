//! WASAPI direto (sem o cpal) para o modo exclusivo: o app fica dono do dispositivo e escolhe o
//! período (3 ms no cabo Rocksmith e no fone desta máquina, contra os 10 ms fixos do compartilhado).
//! Cada lado roda numa thread própria com prioridade "Pro Audio" do MMCSS; a cadeia de efeitos e o
//! anel entre entrada e saída são os mesmos do modo compartilhado.

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use wasapi::{AudioClient, Device, DeviceEnumerator, Direction, SampleType, ShareMode, StreamMode, WasapiError, WaveFormat};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Media::Audio::{
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED,
    AUDCLNT_E_UNSUPPORTED_FORMAT,
};
use windows::Win32::System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW};

use crate::meters::Meters;

pub type CaptureFn = Box<dyn FnMut(&[f32]) + Send>;
pub type RenderFn = Box<dyn FnMut(&mut [f32]) + Send>;

/// O que o dispositivo aceitou.
pub struct StreamInfo {
    pub rate: u32,
    pub channels: usize,
    pub format: String,
}

/// Thread de um lado do áudio; parar = soltar.
pub struct Stream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Dispositivo já aberto e no formato combinado, esperando a callback para começar.
/// Soltar sem chamar `start` fecha o dispositivo.
pub struct Pending<F> {
    pub info: StreamInfo,
    start: Sender<F>,
    stream: Stream,
}

impl<F> Pending<F> {
    pub fn start(self, f: F) -> Stream {
        let Pending { start, stream, .. } = self;
        let _ = start.send(f);
        stream
    }
}

type Opened = Result<StreamInfo, String>;

fn spawn<F: Send + 'static>(
    name: &str,
    body: impl FnOnce(Sender<Opened>, Receiver<F>, Arc<AtomicBool>) + Send + 'static,
) -> Result<Pending<F>, String> {
    let (info_tx, info_rx) = crossbeam_channel::bounded(1);
    let (start_tx, start_rx) = crossbeam_channel::bounded(1);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || body(info_tx, start_rx, thread_stop))
        .map_err(|e| e.to_string())?;
    match info_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(info)) => Ok(Pending { info, start: start_tx, stream: Stream { stop, thread: Some(thread) } }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            // a thread fica solta: se um dia responder, acha o canal fechado e sai
            stop.store(true, Relaxed);
            Err("o dispositivo não respondeu".into())
        }
    }
}

/// Abre a entrada no exclusivo. `period_ms` é o desejado; o driver arredonda para o mínimo dele.
pub fn open_capture(name: &str, period_ms: f64, meters: Arc<Meters>) -> Result<Pending<CaptureFn>, String> {
    let name = name.to_owned();
    spawn("áudio-entrada", move |info_tx, start_rx: Receiver<CaptureFn>, stop| {
        let _ = wasapi::initialize_mta();
        let opened = (|| {
            let dev = find_device(&name, Direction::Capture)?;
            let (client, fmt, period_frames) = init_exclusive(&dev, Direction::Capture, &[48000, 44100], &[1, 2], period_ms)?;
            let event = client.set_get_eventhandle()?;
            let capture = client.get_audiocaptureclient()?;
            Ok::<_, WasapiError>((client, fmt, period_frames, event, capture))
        })();
        let (client, fmt, period_frames, event, capture) = match opened {
            Ok(v) => v,
            Err(e) => {
                let _ = info_tx.send(Err(explain(&e)));
                return;
            }
        };
        let _ = info_tx.send(Ok(info_of(&fmt, period_frames)));
        let Ok(mut callback) = start_rx.recv() else { return };

        let _mmcss = Mmcss::pro_audio();
        let sample = Sample::of(&fmt);
        let block_align = fmt.get_blockalign() as usize;
        let channels = fmt.get_nchannels() as usize;
        let capacity = client.get_buffer_size().unwrap_or(period_frames) as usize * 2;
        let mut bytes = vec![0u8; capacity * block_align];
        let mut samples: Vec<f32> = Vec::with_capacity(capacity * channels);
        if let Err(e) = client.start_stream() {
            log::error!("entrada exclusiva não começou: {e}");
            meters.device_error.store(true, Relaxed);
            return;
        }
        let mut waits = 0;
        let mut packets = 0u32;
        while !stop.load(Relaxed) {
            if event.wait_for_event(100).is_err() {
                waits += 1;
                // 2 s sem nenhum bloco: o cabo saiu ou o driver travou
                if waits >= 20 {
                    log::error!("entrada exclusiva parou de mandar áudio");
                    meters.device_error.store(true, Relaxed);
                    break;
                }
                continue;
            }
            waits = 0;
            // um pacote por evento; ler só com dado pendente evita o ReleaseBuffer de pacote vazio
            match client.get_current_padding() {
                Ok(0) => continue,
                Ok(_) => {}
                Err(e) => {
                    log::error!("entrada exclusiva: {e}");
                    meters.device_error.store(true, Relaxed);
                    break;
                }
            }
            match capture.read_from_device(&mut bytes) {
                Ok((n, info)) => {
                    let n = n as usize;
                    // o primeiro pacote sempre vem marcado (começo do fluxo); depois, cada marca é um buraco
                    packets = packets.saturating_add(1);
                    if info.flags.data_discontinuity && packets > 1 {
                        meters.in_glitches.fetch_add(1, Relaxed);
                    }
                    samples.clear();
                    if info.flags.silent {
                        samples.resize(n * channels, 0.0);
                    } else {
                        samples.extend(bytes[..n * block_align].chunks_exact(sample.bytes()).map(|b| sample.read(b)));
                    }
                    if !samples.is_empty() {
                        callback(&samples);
                    }
                }
                Err(e) => {
                    log::error!("entrada exclusiva: {e}");
                    meters.device_error.store(true, Relaxed);
                    break;
                }
            }
        }
        let _ = client.stop_stream();
    })
}

/// Abre a saída no exclusivo, preferindo a taxa da entrada (assim não há conversão).
/// Por evento: o dispositivo alterna dois buffers de um período e avisa quando um esvazia.
/// (A sondagem foi testada e descartada: o dongle sem fio do JBL Quantum910 não toca nada nela.)
///
/// O mesmo dongle, no exclusivo, às vezes abre pedindo 18 mil amostras/s em vez de 48 mil (com período de 10 ms,
/// 32 mil): toca buracos e repetições, som incompreensível (31/10, medido com `--teste-motor`). Por isso a thread
/// mede o ritmo dos pedidos logo depois de o fone acordar. O defeito é sorteado a cada abertura (01/11: 2 de 6), então
/// ela reabre o fone no exclusivo até duas vezes; se continuar errado, troca para o compartilhado no mesmo
/// dispositivo, sem parar a entrada. `meters.out_fallback` guarda a taxa que o fone pediu.
pub fn open_render(name: &str, rate: u32, period_ms: f64, meters: Arc<Meters>) -> Result<Pending<RenderFn>, String> {
    let name = name.to_owned();
    spawn("áudio-saída", move |info_tx, start_rx: Receiver<RenderFn>, stop| {
        let _ = wasapi::initialize_mta();
        let opened = (|| {
            let dev = find_device(&name, Direction::Render)?;
            let (client, fmt, period_frames) = init_exclusive(&dev, Direction::Render, &[rate, 48000, 44100], &[2], period_ms)?;
            Ok::<_, WasapiError>((dev, client, fmt, period_frames))
        })();
        let (dev, client, fmt, period_frames) = match opened {
            Ok(v) => v,
            Err(e) => {
                let _ = info_tx.send(Err(explain(&e)));
                return;
            }
        };
        let _ = info_tx.send(Ok(info_of(&fmt, period_frames)));
        let Ok(mut callback) = start_rx.recv() else { return };

        let _mmcss = Mmcss::pro_audio();
        let nominal = fmt.get_samplespersec() as f64;
        let mut client = client;
        let mut outcome = render_loop(&client, &fmt, true, &stop, &meters, &mut callback);
        for attempt in 1..=REOPEN_TRIES {
            let Ok(Some(asked)) = outcome else { break };
            log::warn!("saída exclusiva: o dispositivo pediu {asked:.0} amostras/s em vez de {nominal:.0}; reabrindo ({attempt}ª vez)");
            let _ = client.stop_stream();
            drop(client);
            match init_exclusive(&dev, Direction::Render, &[fmt.get_samplespersec()], &[fmt.get_nchannels()], period_ms) {
                Ok((c, f, _)) if f.get_samplespersec() == fmt.get_samplespersec() && f.get_nchannels() == fmt.get_nchannels() => {
                    client = c;
                    outcome = render_loop(&client, &fmt, true, &stop, &meters, &mut callback);
                }
                other => {
                    if let Err(e) = other {
                        log::warn!("saída exclusiva não reabriu: {e}");
                    }
                    // sem o exclusivo de novo: segue para o compartilhado com a taxa errada que ele pediu
                    match dev.get_iaudioclient() {
                        Ok(c) => client = c,
                        Err(e) => {
                            log::error!("saída: {e}");
                            meters.device_error.store(true, Relaxed);
                            meters.out_queue.store(0, Relaxed);
                            return;
                        }
                    }
                    outcome = Ok(Some(asked));
                    break;
                }
            }
        }
        match outcome {
            Ok(Some(asked)) => {
                log::warn!("saída exclusiva: o dispositivo pediu {asked:.0} amostras/s em vez de {nominal:.0}; passando para o compartilhado");
                let _ = client.stop_stream();
                drop(client);
                meters.out_fallback.store(asked as u32, Relaxed);
                // mesmo formato para o resto do motor (canais e taxa); o Windows converte para o mixer dele
                let shared = WaveFormat::new(32, 32, &SampleType::Float, fmt.get_samplespersec() as usize, fmt.get_nchannels() as usize, None);
                let result = dev.get_iaudioclient().and_then(|mut c| {
                    c.initialize_client(&shared, &Direction::Render, &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 })?;
                    Ok(c)
                });
                match result {
                    Ok(c) => {
                        if let Err(e) = render_loop(&c, &shared, false, &stop, &meters, &mut callback) {
                            log::error!("saída compartilhada: {e}");
                            meters.device_error.store(true, Relaxed);
                        }
                        let _ = c.stop_stream();
                    }
                    Err(e) => {
                        log::error!("saída compartilhada não abriu: {e}");
                        meters.device_error.store(true, Relaxed);
                    }
                }
            }
            Ok(None) => {
                let _ = client.stop_stream();
            }
            Err(e) => {
                log::error!("saída exclusiva: {e}");
                meters.device_error.store(true, Relaxed);
                let _ = client.stop_stream();
            }
        }
        meters.out_queue.store(0, Relaxed);
    })
}

/// Laço da saída por evento. No exclusivo escreve um buffer inteiro por evento e confere o ritmo:
/// devolve `Some(taxa pedida)` se o dispositivo pedir a taxa errada. No compartilhado escreve o espaço livre.
fn render_loop(
    client: &AudioClient,
    fmt: &WaveFormat,
    exclusive: bool,
    stop: &AtomicBool,
    meters: &Meters,
    callback: &mut RenderFn,
) -> Result<Option<f64>, WasapiError> {
    let event = client.set_get_eventhandle()?;
    let render = client.get_audiorenderclient()?;
    let sample = Sample::of(fmt);
    let block_align = fmt.get_blockalign() as usize;
    let channels = fmt.get_nchannels() as usize;
    let buffer = client.get_buffer_size()? as usize;
    let mut bytes = vec![0u8; buffer * block_align];
    let mut samples = vec![0.0f32; buffer * channels];
    meters.out_queue.store(if exclusive { 2 * buffer } else { buffer } as u32, Relaxed);

    // o primeiro buffer vai em silêncio, antes de ligar
    render.write_to_device(buffer, &bytes, None)?;
    client.start_stream()?;
    let nominal = fmt.get_samplespersec() as f64;
    // a medida começa no 50º pedido (o fone já acordou) e dura 1 s
    let mut since: Option<std::time::Instant> = None;
    let (mut asked, mut events, mut verified) = (0u64, 0u32, false);
    let mut waits = 0;
    while !stop.load(Relaxed) {
        if event.wait_for_event(100).is_err() {
            waits += 1;
            // o fone sem fio leva ~1 s para acordar; 2 s sem pedido = desligou ou travou
            if waits >= 20 {
                log::error!("saída parou de pedir áudio");
                meters.device_error.store(true, Relaxed);
                break;
            }
            continue;
        }
        waits = 0;
        let want = if exclusive { buffer } else { buffer - client.get_current_padding()? as usize };
        if want == 0 {
            continue;
        }
        let s = &mut samples[..want * channels];
        s.fill(0.0);
        callback(s);
        for (v, b) in s.iter().zip(bytes.chunks_exact_mut(sample.bytes())) {
            sample.write(*v, b);
        }
        render.write_to_device(want, &bytes[..want * block_align], None)?;
        meters.out_frames.fetch_add(want as u64, Relaxed);
        if exclusive && !verified {
            events += 1;
            if let Some(since) = since {
                asked += want as u64;
                let t = since.elapsed().as_secs_f64();
                let rate = asked as f64 / t;
                // o defeito do JBL erra por 33 a 62%: um quarto de segundo já mostra, e cada tentativa toca menos lixo
                if t >= 0.25 && (rate / nominal - 1.0).abs() > 0.15 {
                    return Ok(Some(rate));
                }
                if t >= 1.0 {
                    log::info!("saída exclusiva conferida: {rate:.0} amostras/s (devia ser {nominal:.0})");
                    if (rate / nominal - 1.0).abs() > 0.03 {
                        return Ok(Some(rate));
                    }
                    verified = true;
                }
            } else if events == 50 {
                since = Some(std::time::Instant::now());
            }
        }
    }
    Ok(None)
}

/// Quantas vezes a saída exclusiva é reaberta quando o dispositivo pede a taxa errada, antes do compartilhado.
const REOPEN_TRIES: u32 = 2;

fn find_device(name: &str, dir: Direction) -> Result<Device, WasapiError> {
    DeviceEnumerator::new()?.get_device_collection(&dir)?.get_device_with_name(name)
}

/// Acha um formato que o dispositivo aceite no exclusivo e inicializa por evento com o período pedido.
fn init_exclusive(
    dev: &Device,
    dir: Direction,
    rates: &[u32],
    channels: &[u16],
    period_ms: f64,
) -> Result<(AudioClient, WaveFormat, u32), WasapiError> {
    let mut client = dev.get_iaudioclient()?;
    let mut chans = channels.to_vec();
    chans.push(client.get_mixformat()?.get_nchannels());
    chans.dedup();
    let mut fmt = None;
    'busca: for &r in rates {
        for f in exclusive_candidates(r, &chans) {
            if let Ok(f) = client.is_supported_exclusive_with_quirks(&f) {
                fmt = Some(f);
                break 'busca;
            }
        }
    }
    let fmt = fmt.ok_or(WasapiError::UnsupportedFormat)?;
    let rate = fmt.get_samplespersec() as i64;
    let mode = |period: i64| StreamMode::EventsExclusive { period_hns: period };
    let mut period = client.calculate_aligned_period_near((period_ms * 10_000.0) as i64, None, &fmt)?;
    match client.initialize_client(&fmt, &dir, &mode(period)) {
        Ok(()) => {}
        Err(WasapiError::Windows(e)) if e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => {
            // o driver diz o tamanho alinhado mais próximo; recomeça com ele
            let frames = client.get_buffer_size()? as i64;
            period = wasapi::calculate_period_100ns(frames, rate);
            client = dev.get_iaudioclient()?;
            client.initialize_client(&fmt, &dir, &mode(period))?;
        }
        Err(e) => return Err(e),
    }
    let period_frames = ((period * rate) as f64 / 10_000_000.0).round() as u32;
    log::info!(
        "{} exclusiva: {}, período {period_frames} amostras ({:.2} ms)",
        if dir == Direction::Capture { "entrada" } else { "saída" },
        format_label(&fmt),
        period as f64 / 10_000.0
    );
    Ok((client, fmt, period_frames))
}

fn info_of(fmt: &WaveFormat, period_frames: u32) -> StreamInfo {
    StreamInfo {
        rate: fmt.get_samplespersec(),
        channels: fmt.get_nchannels() as usize,
        format: format!("{}, bloco de {:.1} ms", format_label(fmt), period_frames as f64 * 1000.0 / fmt.get_samplespersec() as f64),
    }
}

fn explain(e: &WasapiError) -> String {
    if let WasapiError::Windows(w) = e {
        let code = w.code();
        if code == AUDCLNT_E_DEVICE_IN_USE {
            return "outro programa está usando o dispositivo".into();
        }
        if code == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED {
            return "o Windows não permite o modo exclusivo nele (Som, Propriedades, Avançado)".into();
        }
        if code == AUDCLNT_E_UNSUPPORTED_FORMAT {
            return "o dispositivo não aceitou nenhum formato".into();
        }
    }
    if matches!(e, WasapiError::UnsupportedFormat) {
        return "o dispositivo não aceitou nenhum formato".into();
    }
    e.to_string()
}

/// Prioridade de áudio do Windows (MMCSS) para a thread atual, enquanto viver.
struct Mmcss(HANDLE);

impl Mmcss {
    fn pro_audio() -> Option<Self> {
        let mut index = 0u32;
        match unsafe { AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &mut index) } {
            Ok(h) => Some(Self(h)),
            Err(e) => {
                log::warn!("sem prioridade de áudio (MMCSS): {e}");
                None
            }
        }
    }
}

impl Drop for Mmcss {
    fn drop(&mut self) {
        let _ = unsafe { AvRevertMmThreadCharacteristics(self.0) };
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sample {
    I16,
    I24,
    /// 32 bits inteiros, inclusive 24 válidos num contêiner de 32 (alinhados à esquerda).
    I32,
    F32,
}

impl Sample {
    fn of(f: &WaveFormat) -> Self {
        match (f.get_subformat(), f.get_bitspersample()) {
            (Ok(SampleType::Float), _) => Self::F32,
            (_, 16) => Self::I16,
            (_, 24) => Self::I24,
            _ => Self::I32,
        }
    }

    fn bytes(self) -> usize {
        match self {
            Self::I16 => 2,
            Self::I24 => 3,
            Self::I32 | Self::F32 => 4,
        }
    }

    fn read(self, b: &[u8]) -> f32 {
        match self {
            Self::I16 => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
            Self::I24 => (i32::from_le_bytes([0, b[0], b[1], b[2]]) as f64 / 2_147_483_648.0) as f32,
            Self::I32 => (i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64 / 2_147_483_648.0) as f32,
            Self::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        }
    }

    fn write(self, v: f32, b: &mut [u8]) {
        let v = v.clamp(-1.0, 1.0);
        match self {
            Self::I16 => b.copy_from_slice(&((v * 32767.0).round() as i16).to_le_bytes()),
            Self::I24 => b.copy_from_slice(&((v as f64 * 8_388_607.0).round() as i32).to_le_bytes()[..3]),
            Self::I32 => b.copy_from_slice(&((v as f64 * 2_147_483_647.0).round() as i32).to_le_bytes()),
            Self::F32 => b.copy_from_slice(&v.to_le_bytes()),
        }
    }
}

fn ms(hns: i64) -> f64 {
    hns as f64 / 10_000.0
}

fn frames_ms(frames: u32, rate: u32) -> f64 {
    frames as f64 * 1000.0 / rate as f64
}

/// Formatos tentados no exclusivo, do preferido para o último recurso.
fn exclusive_candidates(rate: u32, channels: &[u16]) -> Vec<WaveFormat> {
    let mut out = Vec::new();
    for &ch in channels {
        for (bits, valid, kind) in [
            (32, 32, SampleType::Float),
            (32, 24, SampleType::Int),
            (24, 24, SampleType::Int),
            (32, 32, SampleType::Int),
            (16, 16, SampleType::Int),
        ] {
            out.push(WaveFormat::new(bits, valid, &kind, rate as usize, ch as usize, None));
        }
    }
    out
}

fn format_label(f: &WaveFormat) -> String {
    let kind = match f.get_subformat() {
        Ok(SampleType::Float) => "float",
        _ => "int",
    };
    format!(
        "{} ch, {} Hz, {}/{} bits {kind}",
        f.get_nchannels(),
        f.get_samplespersec(),
        f.get_validbitspersample(),
        f.get_bitspersample()
    )
}

/// Texto do `--wasapi`: o que cada dispositivo aceita em cada modo.
pub fn describe() -> String {
    let mut out = String::new();
    if let Err(e) = describe_into(&mut out) {
        let _ = writeln!(out, "erro: {e}");
    }
    out
}

fn describe_into(out: &mut String) -> Result<(), WasapiError> {
    let _ = wasapi::initialize_mta();
    let enumerator = DeviceEnumerator::new()?;
    for dir in [Direction::Capture, Direction::Render] {
        let _ = writeln!(out, "\n{}:", if dir == Direction::Capture { "Entradas" } else { "Saídas" });
        for dev in &enumerator.get_device_collection(&dir)? {
            let dev = dev?;
            let name = dev.get_friendlyname().unwrap_or_default();
            let _ = writeln!(out, "  {name}");
            let client = match dev.get_iaudioclient() {
                Ok(c) => c,
                Err(e) => {
                    let _ = writeln!(out, "    sem cliente de áudio: {e}");
                    continue;
                }
            };
            let mix = client.get_mixformat()?;
            let rate = mix.get_samplespersec();
            let _ = writeln!(out, "    compartilhado (mixer do Windows): {}", format_label(&mix));
            if let Ok((def, min)) = client.get_device_period() {
                let _ = writeln!(out, "    período do dispositivo: padrão {:.2} ms, mínimo {:.2} ms", ms(def), ms(min));
            }
            match client.get_shared_mode_engine_period(&mix) {
                Ok(p) => {
                    let _ = writeln!(
                        out,
                        "    IAudioClient3: padrão {:.2} ms, mínimo {:.2} ms, passo {:.2} ms",
                        frames_ms(p.default, rate),
                        frames_ms(p.min, rate),
                        frames_ms(p.fundamental, rate)
                    );
                }
                Err(e) => {
                    let _ = writeln!(out, "    IAudioClient3: {e}");
                }
            }
            let mut accepted = Vec::new();
            for r in [48000, 44100] {
                for f in exclusive_candidates(r, &[1, 2]) {
                    if client.is_supported(&f, &ShareMode::Exclusive).is_ok() {
                        accepted.push(format_label(&f));
                    }
                }
            }
            if accepted.is_empty() {
                let _ = writeln!(out, "    exclusivo: nenhum dos formatos testados");
            } else {
                let _ = writeln!(out, "    exclusivo aceita: {}", accepted.join(" | "));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_formats_round_trip() {
        for s in [Sample::I16, Sample::I24, Sample::I32, Sample::F32] {
            let mut b = [0u8; 4];
            for v in [-1.0f32, -0.5, -0.001, 0.0, 0.25, 0.999] {
                s.write(v, &mut b[..s.bytes()]);
                let back = s.read(&b[..s.bytes()]);
                assert!((back - v).abs() < 1e-4, "{s:?}: {v} voltou {back}");
            }
        }
    }

    #[test]
    fn sample_write_clamps_instead_of_wrapping() {
        let mut b = [0u8; 2];
        Sample::I16.write(1.7, &mut b);
        assert!(Sample::I16.read(&b) > 0.99);
        Sample::I16.write(-3.0, &mut b);
        assert!(Sample::I16.read(&b) < -0.99);
    }
}
