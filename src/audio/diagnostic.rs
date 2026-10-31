//! `--diagnostico`: toca de verdade (com o preset e os dispositivos salvos) enquanto o Patric toca, e grava três
//! pontos do caminho: o cabo (DI), a saída da cadeia e o que foi mandado ao dispositivo. Comparando os três dá para
//! saber onde o som se estraga: na cadeia em tempo real, no anel entre entrada e saída, ou depois (driver, fone).

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use anyhow::Result;

use super::Engine;
use crate::dsp::SignalChain;
use crate::dsp::cabinet::ImpulseResponse;
use crate::meters::Meters;
use crate::preset::{AppState, Exclusive, LatencyMode, Oversample};

pub fn run(seconds: u64, exclusive: Exclusive, latency: LatencyMode, oversample: Oversample, dir: &Path) -> Result<String> {
    let state = AppState::load();
    let mut settings = state.preset.settings;
    settings.mute = false;
    let meters = Arc::new(Meters::new());
    let mut engine = Engine::start(
        state.input_device.as_deref(),
        state.output_device.as_deref(),
        latency,
        exclusive,
        oversample,
        &settings,
        meters.clone(),
    )?;
    let ir = state.preset.ir_path.as_ref().filter(|_| settings.cab.use_ir).and_then(|p| ImpulseResponse::load(Path::new(p)).ok());
    if let Some(ir) = &ir {
        engine.send_ir(Some(ir.convolver(engine.info.rate)));
    }
    let (rate, out_rate) = (engine.info.rate, engine.info.out_rate);
    let mut out = format!(
        "{} ({rate} Hz, exclusivo {}) -> {} ({out_rate} Hz, exclusivo {}); latência {latency:?}, sobreamostragem {oversample:?}; preset {}\n",
        engine.info.input, engine.info.exclusive.input, engine.info.output, engine.info.exclusive.output, state.preset.name
    );
    if let Some(n) = &engine.info.notice {
        let _ = writeln!(out, "aviso: {n}");
    }

    let mut rec = engine.take_recorder_input().expect("gravador livre");
    let mut tap = engine.take_output_tap().expect("saída livre");
    let n = seconds as usize * rate as usize;
    let (mut dry, mut wet, mut sent) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    // meio segundo para o fone sem fio acordar antes de gravar
    std::thread::sleep(Duration::from_millis(500));
    while tap.pop().is_ok() {}
    meters.underruns.store(0, Relaxed);
    meters.ring_drops.store(0, Relaxed);
    meters.recording.store(true, Relaxed);
    let end = Instant::now() + Duration::from_secs(seconds);
    let (mut fill_min, mut fill_max) = (u32::MAX, 0u32);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
        while rec.slots() >= 2 {
            dry.push(rec.pop().unwrap_or(0.0));
            wet.push(rec.pop().unwrap_or(0.0));
        }
        while let Ok(v) = tap.pop() {
            sent.push(v);
        }
        let f = meters.ring_fill.load(Relaxed);
        fill_min = fill_min.min(f);
        fill_max = fill_max.max(f);
    }
    meters.recording.store(false, Relaxed);
    let _ = writeln!(
        out,
        "blocos: entrada {} amostras, saída {}; maior intervalo entre blocos de entrada {:.1} ms, maior processamento {:.2} ms",
        meters.in_block.load(Relaxed),
        meters.out_block.load(Relaxed),
        meters.in_gap_max_us.load(Relaxed) as f32 / 1000.0,
        meters.proc_max_us.load(Relaxed) as f32 / 1000.0
    );
    let _ = writeln!(
        out,
        "anel entre {fill_min} e {fill_max} amostras; falhas (anel vazio) {}; amostras descartadas (anel cheio) {}; gravador perdeu {}; erro de dispositivo {}",
        meters.underruns.load(Relaxed),
        meters.ring_drops.load(Relaxed),
        meters.rec_dropped.load(Relaxed),
        meters.device_error.load(Relaxed)
    );
    drop(engine);

    std::fs::create_dir_all(dir)?;
    write_wav(&dir.join("1-cabo.wav"), &dry, rate)?;
    write_wav(&dir.join("2-cadeia.wav"), &wet, rate)?;
    write_wav(&dir.join("3-enviado.wav"), &sent, out_rate)?;
    let _ = writeln!(out, "gravado: {} s do cabo, {} s da cadeia, {} s enviados", dry.len() / rate as usize, wet.len() / rate as usize, sent.len() / out_rate as usize);

    // 1) a cadeia em tempo real faz o mesmo que offline?
    let mut chain = SignalChain::new(rate as f32, &settings, oversample.factor());
    if let Some(ir) = &ir {
        chain.cabinet.swap_convolver(Some(ir.convolver(rate)));
    }
    let mut offline = dry.clone();
    let block = (meters.in_block.load(Relaxed) as usize).max(32);
    for b in offline.chunks_mut(block) {
        chain.process(b);
    }
    let skip = rate as usize;
    if wet.len() > skip * 2 {
        let c = correlation(&offline[skip..], &wet[skip..], 0);
        let _ = writeln!(out, "cadeia em tempo real x mesma cadeia offline: correlação {c:.4} (1 = idêntica)");
    }

    // 2) o que foi enviado é a saída da cadeia atrasada, sem buracos?
    let level = rms(&wet);
    let _ = writeln!(out, "nível: cabo {:.1} dBFS, cadeia {:.1} dBFS, enviado {:.1} dBFS", db(rms(&dry)), db(level), db(rms(&sent)));
    if rate == out_rate && sent.len() > skip * 2 && level > 1e-4 {
        // atraso medido no trecho de 2 s mais forte
        let two = 2 * rate as usize;
        let step = rate as usize / 2;
        let start = (skip..wet.len().saturating_sub(two)).step_by(step).max_by(|&a, &b| rms(&wet[a..a + two]).total_cmp(&rms(&wet[b..b + two]))).unwrap_or(skip);
        let seg = &wet[start..start + two.min(wet.len() - start)];
        let lag = (0..rate as usize / 4).max_by(|&a, &b| correlation(&sent[start..], seg, a).total_cmp(&correlation(&sent[start..], seg, b))).unwrap_or(0);
        let _ = writeln!(out, "atraso do anel até o dispositivo: {:.1} ms", lag as f32 * 1000.0 / rate as f32);
        // janelas de 50 ms com som em que o enviado não bate com a cadeia
        let win = rate as usize / 20;
        let (mut bad, mut total, mut holes) = (0, 0, 0);
        let mut worst = Vec::new();
        let mut i = skip;
        while i + win + lag + 400 < sent.len().min(wet.len() + lag) {
            let w = &wet[i..i + win];
            if rms(w) > level * 0.3 {
                total += 1;
                let best = (lag.saturating_sub(200)..lag + 200).map(|l| correlation(&sent[i..], &wet[i..i + win], l)).fold(f32::MIN, f32::max);
                if best < 0.95 {
                    bad += 1;
                    if worst.len() < 8 {
                        worst.push(format!("{:.2} s ({best:.2})", i as f32 / rate as f32));
                    }
                }
                if sent[i + lag..i + lag + win].windows(48).any(|z| z.iter().all(|v| *v == 0.0)) {
                    holes += 1;
                }
            }
            i += win;
        }
        let _ = writeln!(
            out,
            "janelas de 50 ms com som: {total}; enviado diferente da cadeia: {bad}; com buraco de silêncio de 1 ms ou mais: {holes}"
        );
        if !worst.is_empty() {
            let _ = writeln!(out, "primeiras diferentes: {}", worst.join(", "));
        }
    }
    Ok(out)
}

fn write_wav(path: &Path, data: &[f32], rate: u32) -> Result<()> {
    let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &v in data {
        w.write_sample(v)?;
    }
    w.finalize()?;
    Ok(())
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Correlação normalizada entre `a` atrasado de `lag` e `b` (no comprimento de `b`, ou o que couber).
fn correlation(a: &[f32], b: &[f32], lag: usize) -> f32 {
    let n = b.len().min(a.len().saturating_sub(lag));
    if n == 0 {
        return 0.0;
    }
    let (x, y) = (&a[lag..lag + n], &b[..n]);
    let dot: f32 = x.iter().zip(y).map(|(p, q)| p * q).sum();
    let nx: f32 = x.iter().map(|v| v * v).sum::<f32>().sqrt();
    let ny: f32 = y.iter().map(|v| v * v).sum::<f32>().sqrt();
    dot / (nx * ny + 1e-12)
}
