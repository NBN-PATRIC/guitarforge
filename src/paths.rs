use std::path::PathBuf;

/// %APPDATA%\GuitarForge: estado, presets, afinações e log do usuário.
pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("GuitarForge");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn presets_dir() -> PathBuf {
    let dir = app_dir().join("presets");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn state_file() -> PathBuf {
    app_dir().join("estado.json")
}

pub fn tunings_file() -> PathBuf {
    app_dir().join("afinacoes.json")
}

pub fn log_file() -> PathBuf {
    app_dir().join("guitarforge.log")
}

/// %USERPROFILE%\Music\GuitarForge (criada só quando a primeira gravação começa).
pub fn recordings_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Music"))
        .unwrap_or_else(app_dir)
        .join("GuitarForge")
}
