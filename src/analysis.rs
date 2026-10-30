use std::collections::VecDeque;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::dsp::hum_filter::HumFilter;
use crate::dsp::{hum_meter, lin_to_db, tuner};
use crate::meters::Meters;

// guitarra: 4096 amostras (85 ms a 48 kHz); baixo precisa de janela maior para pegar o Si grave de 31 Hz
const TUNER_WINDOW: usize = 4096;
const BASS_WINDOW: usize = 8192;
const LEVEL_WINDOW: usize = 2048;
const SILENCE_DB: f32 = -50.0;

/// Thread de análise: afinador, nível, ruído de fundo e zumbido de 60 Hz.
/// Fica fora da thread de áudio porque o YIN é caro. Termina sozinha quando o motor é desligado.
pub fn spawn(rx: rtrb::Consumer<f32>, sr: f32, meters: Arc<Meters>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("analise".into())
        .spawn(move || run(rx, sr, &meters))
        .expect("thread de análise")
}

fn run(mut rx: rtrb::Consumer<f32>, sr: f32, m: &Meters) {
    let hist_len = sr as usize;
    let mut hist = vec![0.0f32; hist_len];
    // o afinador lê o sinal sem o zumbido de 60 Hz (o medidor de zumbido continua lendo o sinal cru)
    let mut clean = vec![0.0f32; hist_len];
    let mut hum = HumFilter::new(sr, m.mains_hz.get());
    let mut sample = [0.0f32; 1];
    let mut pos = 0usize;
    let mut filled = 0usize;
    let mut window = Vec::with_capacity(BASS_WINDOW);
    let mut decimated = Vec::with_capacity(BASS_WINDOW / 2);
    let mut levels: VecDeque<f32> = VecDeque::with_capacity(128);
    let mut last_tuner = Instant::now();
    let mut last_hum = Instant::now();
    let mut last_pitch = Instant::now() - Duration::from_secs(10);

    loop {
        while let Ok(s) = rx.pop() {
            sample[0] = s;
            hum.process(&mut sample);
            clean[pos] = sample[0];
            hist[pos] = s;
            pos = (pos + 1) % hist_len;
            filled = (filled + 1).min(hist_len);
        }
        if rx.is_abandoned() {
            break;
        }
        let now = Instant::now();

        let fmin = m.pitch_fmin.get().clamp(20.0, 60.0);
        let target = Some(m.pitch_target.get()).filter(|t| *t > 0.0);
        let lowest = target.map_or(fmin, |t| fmin.min(t * 2f32.powf(-tuner::TARGET_BELOW / 12.0)));
        let len = if lowest < 40.0 { BASS_WINDOW } else { TUNER_WINDOW };
        if now - last_tuner >= Duration::from_millis(50) && filled >= len {
            last_tuner = now;
            window.clear();
            let start = (pos + hist_len - len) % hist_len;
            window.extend((0..len).map(|i| hist[(start + i) % hist_len]));

            let tail = &window[len - LEVEL_WINDOW..];
            let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
            let level_db = lin_to_db(rms);
            m.in_rms_db.set(level_db);
            if levels.len() == 100 {
                levels.pop_front();
            }
            levels.push_back(level_db);
            m.noise_floor_db.set(levels.iter().copied().fold(f32::INFINITY, f32::min));

            if level_db > SILENCE_DB {
                window.clear();
                window.extend((0..len).map(|i| clean[(start + i) % hist_len]));
                decimated.clear();
                decimated.extend(window.chunks_exact(2).map(|p| 0.5 * (p[0] + p[1])));
                if let Some(f) = tuner::detect(&decimated, sr / 2.0, fmin, 1100.0, target) {
                    m.pitch_hz.set(f);
                    last_pitch = now;
                }
            }
            if now - last_pitch > Duration::from_millis(700) {
                m.pitch_hz.set(0.0);
            }
        }

        if now - last_hum >= Duration::from_millis(500) && filled == hist_len {
            last_hum = now;
            // Brasil é 60 Hz; se o cabo mostrar zumbido claramente de 50 Hz (Europa, boa parte do mundo), troca
            let at50 = hum_meter::tone_level_db(&hist, sr, 50.0);
            let at60 = hum_meter::tone_level_db(&hist, sr, 60.0);
            let mut mains = m.mains_hz.get();
            if at50 > at60 + 6.0 && at50 > -90.0 {
                mains = 50.0;
            } else if at60 > at50 + 6.0 && at60 > -90.0 {
                mains = 60.0;
            }
            m.mains_hz.set(mains);
            hum.retune(mains);
            m.hum_db.set(if mains == 50.0 { at50 } else { at60 });
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Mesmo afinador da thread de análise, rodando sobre um WAV (diagnóstico: `guitarforge --analisar arquivo.wav`).
/// Devolve (segundos, Hz detectado ou nada, nível dBFS) a cada 50 ms.
pub fn analyze_file(path: &std::path::Path, fmin: f32, target: Option<f32>) -> anyhow::Result<Vec<(f32, Option<f32>, f32)>> {
    let (mono, sr) = read_mono(path)?;
    let mut clean = mono.clone();
    HumFilter::new(sr, crate::dsp::hum_filter::DEFAULT_MAINS_HZ).process(&mut clean);
    let len = if fmin < 50.0 { BASS_WINDOW } else { TUNER_WINDOW };
    let hop = (sr * 0.05) as usize;
    let mut out = Vec::new();
    let mut decimated = Vec::with_capacity(len / 2);
    let mut end = len;
    while end <= mono.len() {
        let window = &mono[end - len..end];
        let filtered = &clean[end - len..end];
        let tail = &window[len - LEVEL_WINDOW..];
        let level = lin_to_db((tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt());
        let hz = if level > SILENCE_DB {
            decimated.clear();
            decimated.extend(filtered.chunks_exact(2).map(|p| 0.5 * (p[0] + p[1])));
            tuner::detect(&decimated, sr / 2.0, fmin, 1100.0, target)
        } else {
            None
        };
        out.push((end as f32 / sr, hz, level));
        end += hop;
    }
    Ok(out)
}

fn read_mono(path: &std::path::Path) -> anyhow::Result<(Vec<f32>, f32)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(8, 32) - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    Ok((raw.chunks(channels).map(|f| f[0]).collect(), spec.sample_rate as f32))
}

/// Picos do espectro (40 a 700 Hz) de um trecho do WAV: `guitarforge --espectro arquivo.wav inicio fim`.
/// Devolve (Hz, dB relativo ao maior pico), ordenado por frequência.
pub fn spectrum_peaks(path: &std::path::Path, start: f32, end: f32) -> anyhow::Result<Vec<(f32, f32)>> {
    let (mono, sr) = read_mono(path)?;
    let a = ((start * sr) as usize).min(mono.len());
    let b = ((end * sr) as usize).clamp(a, mono.len());
    let seg = &mono[a..b];
    let n = 1usize << 18;
    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let mut input = fft.make_input_vec();
    let len = seg.len().max(1) as f32;
    for (i, (dst, &x)) in input.iter_mut().zip(seg).enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / len).cos();
        *dst = x * w;
    }
    let mut spectrum = fft.make_output_vec();
    fft.process(&mut input, &mut spectrum)?;
    let mag: Vec<f32> = spectrum.iter().map(|c| c.norm()).collect();
    let bin = sr / n as f32;
    let (lo, hi) = ((40.0 / bin) as usize, (700.0 / bin) as usize);
    let max = mag[lo..hi].iter().copied().fold(0.0f32, f32::max).max(1e-12);
    let mut peaks: Vec<(f32, f32)> = (lo.max(1)..hi)
        .filter(|&i| mag[i] > mag[i - 1] && mag[i] >= mag[i + 1] && mag[i] > 0.05 * max)
        .map(|i| (i as f32 * bin, 20.0 * (mag[i] / max).log10()))
        .collect();
    peaks.sort_by(|x, y| y.1.total_cmp(&x.1));
    peaks.truncate(8);
    peaks.sort_by(|x, y| x.0.total_cmp(&y.0));
    Ok(peaks)
}

/// Pico e RMS de um trecho, cru e depois do filtro de zumbido (dBFS): `guitarforge --ruido arquivo.wav inicio fim`.
pub fn noise_report(path: &std::path::Path, start: f32, end: f32) -> anyhow::Result<[(f32, f32); 2]> {
    let (mono, sr) = read_mono(path)?;
    let mut clean = mono.clone();
    HumFilter::new(sr, crate::dsp::hum_filter::DEFAULT_MAINS_HZ).process(&mut clean);
    let a = ((start * sr) as usize).min(mono.len());
    let b = ((end * sr) as usize).clamp(a, mono.len());
    let stats = |x: &[f32]| {
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rms = (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt();
        (lin_to_db(peak), lin_to_db(rms))
    };
    Ok([stats(&mono[a..b]), stats(&clean[a..b])])
}
