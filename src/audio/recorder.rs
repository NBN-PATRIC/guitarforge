use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::meters::Meters;

type Writer = hound::WavWriter<std::io::BufWriter<std::fs::File>>;

/// Gravação em andamento: dois WAV float 32 bits, o som processado e o sinal seco do cabo (DI).
pub struct Recording {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<(rtrb::Consumer<f32>, Result<()>)>,
    pub path: PathBuf,
    pub started: Instant,
}

fn next_free_name(dir: &Path) -> (PathBuf, PathBuf) {
    (1..)
        .map(|n| (dir.join(format!("GuitarForge {n:03}.wav")), dir.join(format!("GuitarForge {n:03} (DI).wav"))))
        .find(|(wet, _)| !wet.exists())
        .expect("sempre há um número livre")
}

pub fn start(
    mut rx: rtrb::Consumer<f32>,
    rate: u32,
    dir: &Path,
    meters: Arc<Meters>,
) -> std::result::Result<Recording, (rtrb::Consumer<f32>, anyhow::Error)> {
    let created = (|| -> Result<(PathBuf, Writer, Writer)> {
        std::fs::create_dir_all(dir).with_context(|| format!("criando {}", dir.display()))?;
        let (wet_path, dry_path) = next_free_name(dir);
        let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let wet = hound::WavWriter::create(&wet_path, spec)?;
        let dry = hound::WavWriter::create(&dry_path, spec)?;
        Ok((wet_path, wet, dry))
    })();
    let (path, mut wet, mut dry) = match created {
        Ok(v) => v,
        Err(e) => return Err((rx, e)),
    };

    while rx.pop().is_ok() {}
    meters.rec_dropped.store(0, Relaxed);
    meters.recording.store(true, Relaxed);

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let handle = std::thread::Builder::new()
        .name("gravador".into())
        .spawn(move || {
            let mut result = Ok(());
            loop {
                while rx.slots() >= 2 {
                    if let (Ok(d), Ok(w)) = (rx.pop(), rx.pop()) {
                        if result.is_ok() {
                            result = dry.write_sample(d).and_then(|_| wet.write_sample(w)).map_err(anyhow::Error::from);
                        }
                    }
                }
                if stop_flag.load(Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let finished = wet.finalize().and(dry.finalize()).map_err(anyhow::Error::from);
            (rx, result.and(finished))
        })
        .expect("thread do gravador");

    Ok(Recording { stop, handle, path, started: Instant::now() })
}

impl Recording {
    pub fn stop(self, meters: &Meters) -> (Option<rtrb::Consumer<f32>>, Result<PathBuf>) {
        meters.recording.store(false, Relaxed);
        self.stop.store(true, Relaxed);
        match self.handle.join() {
            Ok((rx, result)) => (Some(rx), result.map(|_| self.path)),
            Err(_) => (None, Err(anyhow::anyhow!("o gravador parou de forma inesperada"))),
        }
    }
}
