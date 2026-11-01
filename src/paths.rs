use std::path::{Path, PathBuf};

/// Grava num arquivo ao lado e troca de nome no fim: se o app cair no meio, o arquivo antigo continua inteiro
/// (antes o estado podia ficar pela metade e o app voltava ao padrão, perdendo dispositivos e preset).
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".novo");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

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

/// %APPDATA%\GuitarForge\irs: cópia de cada IR carregado. O preset aponta para a cópia, não para o original.
pub fn irs_dir() -> PathBuf {
    let dir = app_dir().join("irs");
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
