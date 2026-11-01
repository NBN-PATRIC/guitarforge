#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod audio;
mod dsp;
mod gui;
mod instance;
mod meters;
mod paths;
mod preset;
mod settings;

fn init_log() {
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("guitarforge=info,warn"));
    // sem console na versão release: o log vai para %APPDATA%\GuitarForge\guitarforge.log
    if !cfg!(debug_assertions)
        && let Ok(file) = std::fs::File::create(paths::log_file())
    {
        builder.target(env_logger::Target::Pipe(Box::new(file)));
    }
    builder.init();
    std::panic::set_hook(Box::new(|info| log::error!("pânico: {info}")));
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // a janela (sem opção de diagnóstico) é uma só; olhar antes do log, que a segunda cópia apagaria ao criá-lo
    let window = !args.iter().skip(1).any(|a| a.starts_with("--"));
    let instance = if window {
        match instance::claim() {
            instance::Claim::Second => return Ok(()),
            instance::Claim::First(i) => i,
        }
    } else {
        None
    };
    init_log();
    log::info!("GuitarForge v{}", env!("CARGO_PKG_VERSION"));

    if args.iter().any(|a| a == "--dispositivos") {
        println!("{}", audio::describe());
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--print") {
        // --print arquivo.bmp [largura altura] [fim]
        let path = args.get(i + 1).ok_or_else(|| anyhow::anyhow!("uso: --print arquivo.bmp [largura altura]"))?;
        let w = args.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(1220.0);
        let h = args.get(i + 3).and_then(|v| v.parse().ok()).unwrap_or(860.0);
        let bottom = args.get(i + 4).is_some_and(|v| v == "fim");
        return gui::print(path.into(), [w, h], bottom);
    }
    if let Some(i) = args.iter().position(|a| a == "--diagnostico") {
        // --diagnostico segundos [entrada|saida|ambos|nenhum] [baixa|normal|segura] [1|2|4] pasta
        let seconds = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(20);
        let which = args.get(i + 2).map(String::as_str).unwrap_or("ambos");
        let exclusive = preset::Exclusive {
            input: matches!(which, "entrada" | "ambos"),
            output: matches!(which, "saida" | "ambos"),
        };
        let latency = match args.get(i + 3).map(String::as_str) {
            Some("baixa") => preset::LatencyMode::Low,
            Some("segura") => preset::LatencyMode::Safe,
            _ => preset::LatencyMode::Normal,
        };
        let oversample = match args.get(i + 4).map(String::as_str) {
            Some("2") => preset::Oversample::X2,
            Some("4") => preset::Oversample::X4,
            _ => preset::Oversample::Off,
        };
        let dir = args.get(i + 5).map(std::path::PathBuf::from).unwrap_or_else(|| "diagnostico".into());
        print!("{}", audio::diagnostic::run(seconds, exclusive, latency, oversample, &dir)?);
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--teste-motor") {
        // --teste-motor [segundos] [entrada|saida|ambos|nenhum] [baixa|normal|segura] [nome da saída]
        let seconds = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(10);
        let which = args.get(i + 2).map(String::as_str).unwrap_or("ambos");
        let exclusive = preset::Exclusive {
            input: matches!(which, "entrada" | "ambos"),
            output: matches!(which, "saida" | "ambos"),
        };
        let latency = match args.get(i + 3).map(String::as_str) {
            Some("baixa") => preset::LatencyMode::Low,
            Some("segura") => preset::LatencyMode::Safe,
            _ => preset::LatencyMode::Normal,
        };
        let output = args.get(i + 4).map(String::as_str);
        print!("{}", audio::engine_test(seconds, exclusive, latency, output)?);
        return Ok(());
    }
    #[cfg(windows)]
    if args.iter().any(|a| a == "--wasapi") {
        println!("{}", audio::wasapi_io::describe());
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--ruido") {
        let usage = || anyhow::anyhow!("uso: --ruido arquivo.wav inicio_s fim_s");
        let path = args.get(i + 1).ok_or_else(usage)?;
        let start: f32 = args.get(i + 2).and_then(|v| v.parse().ok()).ok_or_else(usage)?;
        let end: f32 = args.get(i + 3).and_then(|v| v.parse().ok()).ok_or_else(usage)?;
        let [(rp, rr), (fp, fr)] = analysis::noise_report(std::path::Path::new(path), start, end)?;
        println!("cru: pico {rp:.1} dBFS, RMS {rr:.1} | sem 60 Hz: pico {fp:.1} dBFS, RMS {fr:.1}");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--espectro") {
        let usage = || anyhow::anyhow!("uso: --espectro arquivo.wav inicio_s fim_s");
        let path = args.get(i + 1).ok_or_else(usage)?;
        let start: f32 = args.get(i + 2).and_then(|v| v.parse().ok()).ok_or_else(usage)?;
        let end: f32 = args.get(i + 3).and_then(|v| v.parse().ok()).ok_or_else(usage)?;
        let peaks = analysis::spectrum_peaks(std::path::Path::new(path), start, end)?;
        let text: Vec<String> = peaks.iter().map(|(f, db)| format!("{f:.1} Hz ({db:+.0} dB)")).collect();
        println!("{start:.2}-{end:.2} s: {}", text.join(", "));
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--analisar") {
        let path = args.get(i + 1).ok_or_else(|| anyhow::anyhow!("uso: --analisar arquivo.wav [fmin] [Hz da corda escolhida]"))?;
        let fmin = args.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(60.0);
        let target = args.get(i + 3).and_then(|v| v.parse().ok());
        for (t, hz, level) in analysis::analyze_file(std::path::Path::new(path), fmin, target)? {
            match hz {
                Some(f) => {
                    let n = dsp::tuner::note_for(f);
                    println!("{t:7.2} s  {level:6.1} dBFS  {f:8.2} Hz  {}{} {:+.0}", dsp::tuner::Notation::Letters.name(n.index), n.octave, n.cents);
                }
                None => println!("{t:7.2} s  {level:6.1} dBFS  --"),
            }
        }
        return Ok(());
    }
    gui::run(instance)
}
