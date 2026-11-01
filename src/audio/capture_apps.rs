//! Programas que transmitem, chamam ou gravam o som do PC (OBS, Discord, Teams…). Com a saída exclusiva o som do
//! GuitarForge passa por fora do mixer do Windows: nenhum deles ouve nem captura a guitarra, e o som deles naquela
//! saída fica mudo. Medido em 01/11: a captura do dispositivo (Áudio do desktop do OBS) nem abre, "dispositivo em
//! uso"; a captura por aplicativo só pega zeros; o Chrome, ao compartilhar a tela com som, entrega um tom de 440 Hz no
//! lugar do som do sistema, e o AudioContext dele naquela saída fica parado até reabrir.
//!
//! O navegador fica de fora: está sempre aberto e não dá para saber de fora se ele está transmitindo. Para ele valem o
//! aviso do diálogo Áudio… e a marca "saída exclusiva" no topo.

/// Executável (minúsculo) e o nome mostrado.
const KNOWN: &[(&str, &str)] = &[
    ("obs64.exe", "OBS"),
    ("obs32.exe", "OBS"),
    ("obs.exe", "OBS"),
    ("streamlabs obs.exe", "Streamlabs"),
    ("streamlabs desktop.exe", "Streamlabs"),
    ("xsplit.core.exe", "XSplit"),
    ("discord.exe", "Discord"),
    ("discordptb.exe", "Discord"),
    ("discordcanary.exe", "Discord"),
    ("ms-teams.exe", "Teams"),
    ("teams.exe", "Teams"),
    ("zoom.exe", "Zoom"),
    ("audacity.exe", "Audacity"),
];

/// Os programas conhecidos que estão abertos agora, sem repetir, na ordem da lista.
pub fn running() -> Vec<&'static str> {
    known_in(&process_names())
}

fn known_in(names: &[String]) -> Vec<&'static str> {
    let mut found = Vec::new();
    for (exe, app) in KNOWN {
        if !found.contains(app) && names.iter().any(|n| n == exe) {
            found.push(*app);
        }
    }
    found
}

/// Nomes dos executáveis de todos os processos (minúsculos). Uma foto da lista leva poucos milissegundos.
#[cfg(windows)]
fn process_names() -> Vec<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    let mut out = Vec::new();
    // SAFETY: a foto é fechada no fim; a entrada leva o tamanho certo em dwSize, como a API pede
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let name = &entry.szExeFile;
            let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            out.push(String::from_utf16_lossy(&name[..len]).to_lowercase());
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
fn process_names() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_apps_once_in_list_order() {
        let names: Vec<String> =
            ["chrome.exe", "discord.exe", "obs64.exe", "discord.exe", "discordptb.exe"].iter().map(|s| s.to_string()).collect();
        assert_eq!(known_in(&names), ["OBS", "Discord"]);
        assert!(known_in(&["chrome.exe".to_string(), "msedge.exe".to_string()]).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn sees_own_process() {
        let names = process_names();
        assert!(names.iter().any(|n| n.starts_with("guitarforge")), "{} processos, sem o próprio teste", names.len());
    }
}
