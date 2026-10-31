mod theme;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Button, Color32, ComboBox, Frame, Key, Label, Layout, Margin, RichText, Rounding, Stroke, Ui,
    vec2,
};

use crate::audio::recorder::{self, Recording};
use crate::audio::{self, Engine};
use crate::dsp::cabinet::ImpulseResponse;
use crate::dsp::lin_to_db;
use crate::dsp::tuner::{self, Instrument, Tuning};
use crate::meters::Meters;
use crate::paths;
use crate::preset::{self, AppState, Exclusive, LatencyMode, Oversample, Preset};
use crate::settings::{AmpChannel, CabKind, DriveKind, EQ_FREQS, Settings};
use widgets::{KNOB_W, Knob, fmt_db, fmt_ms};

/// Largura máxima da coluna de conteúdo: maximizado, o rack fica centralizado em vez de esticar.
const MAX_W: f32 = 1360.0;
const MIN_SIDE: f32 = 16.0;
const GAP: f32 = 12.0;
const CARD_PAD: f32 = 12.0;
const LABEL_W: f32 = 64.0;

pub fn run() -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GuitarForge")
            .with_inner_size([1220.0, 860.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_icon(Arc::new(
                eframe::icon_data::from_png_bytes(include_bytes!("../../assets/guitarforge.png")).expect("ícone embutido"),
            )),
        ..Default::default()
    };
    eframe::run_native(
        "GuitarForge",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new(true)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// `--print`: abre a janela sem áudio, deixa assentar, grava o que o próprio app desenhou num BMP e fecha.
/// Serve para conferir o layout sem capturar a tela (que pegaria as janelas de outros programas).
pub fn print(path: PathBuf, size: [f32; 2], bottom: bool) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("GuitarForge (print)").with_inner_size(size),
        ..Default::default()
    };
    eframe::run_native(
        "GuitarForge print",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let mut app = App::new(false);
            app.print = Some((path, 0));
            app.print_bottom = bottom;
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

fn save_bmp(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        let start = out.len();
        for p in &image.pixels[y * w..(y + 1) * w] {
            out.extend_from_slice(&[p.b(), p.g(), p.r()]);
        }
        out.resize(start + row, 0);
    }
    std::fs::write(path, out)
}

struct TuningEditor {
    original: Option<String>,
    tuning: Tuning,
    error: Option<String>,
    confirm_delete: bool,
}

struct AudioDialog {
    input: Option<String>,
    output: Option<String>,
    latency: LatencyMode,
    exclusive: Exclusive,
    oversample: Oversample,
}

enum CabAction {
    None,
    Load,
    Remove,
}

struct App {
    state: AppState,
    baseline: Settings,
    sent: Option<Settings>,
    last_seen: Settings,
    meters: Arc<Meters>,
    engine: Option<Engine>,
    engine_error: Option<String>,
    devices: audio::Devices,
    ir: Option<ImpulseResponse>,
    ir_error: Option<String>,
    recording: Option<Recording>,
    last_recording: Option<PathBuf>,
    rec_error: Option<String>,
    factory: Vec<Preset>,
    user: Vec<Preset>,
    save_name: Option<String>,
    save_error: Option<String>,
    audio_dialog: Option<AudioDialog>,
    tuning: bool,
    tunings: Vec<Tuning>,
    tuning_editor: Option<TuningEditor>,
    auto_strings: bool,
    tracker: tuner::StringTracker,
    epoch: Instant,
    string_lock: Option<usize>,
    string_current: Option<usize>,
    strings_done: Vec<bool>,
    in_tune_since: Option<Instant>,
    in_db: f32,
    in_hold: f32,
    in_hold_at: Instant,
    out_db: f32,
    clip_until: Instant,
    needle: f32,
    dirty: bool,
    last_save: Instant,
    /// `--print`: destino e quadros desenhados até agora.
    print: Option<(PathBuf, u32)>,
    print_bottom: bool,
}

impl App {
    fn new(audio: bool) -> Self {
        let state = AppState::load();
        let now = Instant::now();
        let mut app = Self {
            baseline: state.preset.settings,
            last_seen: state.preset.settings,
            state,
            sent: None,
            meters: Arc::new(Meters::new()),
            engine: None,
            engine_error: None,
            devices: audio::Devices { inputs: Vec::new(), outputs: Vec::new() },
            ir: None,
            ir_error: None,
            recording: None,
            last_recording: None,
            rec_error: None,
            factory: preset::factory(),
            user: preset::user_presets(),
            save_name: None,
            save_error: None,
            audio_dialog: None,
            tuning: false,
            tunings: all_tunings(),
            tuning_editor: None,
            auto_strings: true,
            tracker: tuner::StringTracker::default(),
            epoch: now,
            string_lock: None,
            string_current: None,
            strings_done: Vec::new(),
            in_tune_since: None,
            in_db: -90.0,
            in_hold: -90.0,
            in_hold_at: now,
            out_db: -90.0,
            clip_until: now,
            needle: 0.0,
            dirty: false,
            last_save: now,
            print: None,
            print_bottom: false,
        };
        if let Some(path) = app.state.preset.ir_path.clone() {
            app.load_ir(PathBuf::from(path), false);
        }
        app.reset_strings();
        if audio {
            app.start_engine();
        }
        app
    }

    fn effective_settings(&self) -> Settings {
        let mut s = self.state.preset.settings;
        if self.tuning {
            s.mute = true;
        }
        s
    }

    fn start_engine(&mut self) {
        self.stop_recording();
        self.engine = None;
        self.sent = None;
        let settings = self.effective_settings();
        match Engine::start(
            self.state.input_device.as_deref(),
            self.state.output_device.as_deref(),
            self.state.latency,
            self.state.exclusive,
            self.state.oversample,
            &settings,
            self.meters.clone(),
        ) {
            Ok(engine) => {
                if let Some(ir) = &self.ir {
                    engine.send_ir(Some(ir.convolver(engine.info.rate)));
                }
                self.sent = Some(settings);
                self.engine = Some(engine);
                self.engine_error = None;
            }
            Err(e) => {
                log::error!("áudio: {e:#}");
                self.engine_error = Some(format!("{e:#}"));
            }
        }
    }

    fn load_ir(&mut self, path: PathBuf, enable: bool) {
        match ImpulseResponse::load(&path) {
            Ok(ir) => {
                if let Some(e) = &self.engine {
                    e.send_ir(Some(ir.convolver(e.info.rate)));
                }
                self.state.preset.ir_path = Some(path.to_string_lossy().into_owned());
                if enable {
                    self.state.preset.settings.cab.on = true;
                    self.state.preset.settings.cab.use_ir = true;
                }
                self.ir = Some(ir);
                self.ir_error = None;
                self.dirty = true;
            }
            Err(e) => self.ir_error = Some(format!("{e:#}")),
        }
    }

    fn pick_ir(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Escolha um IR de gabinete").add_filter("Arquivo WAV", &["wav"]);
        if let Some(dir) = self.ir.as_ref().and_then(|ir| ir.path.parent().map(|p| p.to_path_buf())) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.load_ir(path, true);
        }
    }

    fn remove_ir(&mut self) {
        self.ir = None;
        self.ir_error = None;
        self.state.preset.ir_path = None;
        self.state.preset.settings.cab.use_ir = false;
        if let Some(e) = &self.engine {
            e.send_ir(None);
        }
        self.dirty = true;
    }

    fn apply_preset(&mut self, mut p: Preset) {
        let current = self.state.preset.settings;
        p.settings.master = current.master;
        p.settings.mute = current.mute;
        p.settings.input_gain_db = current.input_gain_db;
        // a supressão de ruído é do cabo e da guitarra, não do som: não muda com o preset
        p.settings.gate = current.gate;
        self.baseline = p.settings;
        let ir_path = p.ir_path.clone();
        self.state.preset = p;
        match ir_path {
            Some(path) => self.load_ir(PathBuf::from(path), false),
            None => self.state.preset.ir_path = self.ir.as_ref().map(|ir| ir.path.to_string_lossy().into_owned()),
        }
        if let Some(id) = self.state.preset.tuning.clone() {
            if self.tunings.iter().any(|t| t.id() == id) {
                self.select_tuning(id);
            }
        }
        self.dirty = true;
    }

    fn save_user_preset(&mut self, name: &str) -> Result<(), String> {
        if name.is_empty() {
            return Err("Dê um nome ao preset.".into());
        }
        if self.factory.iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
            return Err("Esse nome é de um preset de fábrica, escolha outro.".into());
        }
        let mut p = self.state.preset.clone();
        p.name = name.to_string();
        p.factory = false;
        p.tuning = Some(self.current_tuning().id());
        p.save(&preset::user_preset_path(name)).map_err(|e| format!("{e:#}"))?;
        self.baseline = p.settings;
        self.state.preset = p;
        self.user = preset::user_presets();
        self.dirty = true;
        Ok(())
    }

    fn toggle_recording(&mut self) {
        if self.recording.is_some() {
            self.stop_recording();
            return;
        }
        let Some(engine) = self.engine.as_mut() else { return };
        let Some(rx) = engine.take_recorder_input() else { return };
        match recorder::start(rx, engine.info.rate, &paths::recordings_dir(), self.meters.clone()) {
            Ok(r) => {
                self.recording = Some(r);
                self.rec_error = None;
            }
            Err((rx, e)) => {
                engine.return_recorder_input(rx);
                self.rec_error = Some(format!("{e:#}"));
            }
        }
    }

    fn stop_recording(&mut self) {
        let Some(rec) = self.recording.take() else { return };
        let (rx, result) = rec.stop(&self.meters);
        if let (Some(rx), Some(engine)) = (rx, self.engine.as_mut()) {
            engine.return_recorder_input(rx);
        }
        match result {
            Ok(path) => {
                self.last_recording = Some(path);
                self.rec_error = None;
            }
            Err(e) => self.rec_error = Some(format!("{e:#}")),
        }
    }

    fn open_audio_dialog(&mut self) {
        self.devices = audio::list_devices();
        self.audio_dialog = Some(AudioDialog {
            input: self.state.input_device.clone(),
            output: self.state.output_device.clone(),
            latency: self.state.latency,
            exclusive: self.state.exclusive,
            oversample: self.state.oversample,
        });
    }

    fn poll(&mut self) {
        let fallback = self.meters.out_fallback.load(Relaxed);
        if let Some(e) = self.engine.as_mut().filter(|e| fallback > 0 && e.info.exclusive.output) {
            // a thread da saída já trocou sozinha para o compartilhado; aqui só fica registrado e visível
            e.info.exclusive.output = false;
            e.info.notice = Some(format!(
                "o fone pediu {fallback} amostras/s em vez de {}, defeito dele no modo exclusivo",
                e.info.out_rate
            ));
        }
        if self.engine.is_some() && self.meters.device_error.swap(false, Relaxed) {
            self.stop_recording();
            self.engine = None;
            self.engine_error = Some("O dispositivo de áudio parou de responder (cabo desconectado?).".into());
        }
        if let Some(e) = &self.engine {
            e.collect_trash();
        }

        let now = Instant::now();
        let in_peak = self.meters.in_peak.take();
        let in_db = lin_to_db(in_peak);
        self.in_db = if in_db > self.in_db { in_db } else { (self.in_db - 1.5).max(-90.0) };
        if in_db >= self.in_hold || now - self.in_hold_at > Duration::from_millis(1500) {
            self.in_hold = in_db;
            self.in_hold_at = now;
        }
        if in_peak >= 0.98 {
            self.clip_until = now + Duration::from_millis(1500);
        }
        let out_db = lin_to_db(self.meters.out_peak.take());
        self.out_db = if out_db > self.out_db { out_db } else { (self.out_db - 1.5).max(-90.0) };

        let lowest = self.current_tuning().lowest_freq();
        self.meters.pitch_fmin.set((lowest * 0.7).clamp(20.0, 60.0));
        let target = self.string_lock.and_then(|i| self.current_tuning().midi.get(i).copied()).map(tuner::midi_freq);
        self.meters.pitch_target.set(target.unwrap_or(0.0));
        if self.strings_done.len() != self.current_tuning().midi.len() {
            self.reset_strings();
        }
        let hz = self.meters.pitch_hz.get();
        if hz > 0.0 {
            let tuning = self.current_tuning().clone();
            let t = (now - self.epoch).as_secs_f64();
            let current = match self.string_lock {
                Some(l) => Some(l),
                None => self.tracker.update(hz, t, &tuning),
            };
            self.string_current = current;
            let cents = match current {
                Some(i) => tuner::cents_between(hz, tuner::midi_freq(tuning.midi[i])),
                None => tuner::note_for(hz).cents,
            };
            self.needle += (cents.clamp(-60.0, 60.0) - self.needle) * 0.35;
            match current {
                Some(i) if cents.abs() <= 5.0 => {
                    let since = *self.in_tune_since.get_or_insert(now);
                    if now - since > Duration::from_millis(700) && !self.strings_done[i] {
                        self.strings_done[i] = true;
                        self.in_tune_since = None;
                        // travado numa corda: afinou, passa para a próxima que falta (da mais grave para a mais aguda)
                        if self.string_lock == Some(i) {
                            let n = self.strings_done.len();
                            self.string_lock = (1..n).map(|k| (i + k) % n).find(|&j| !self.strings_done[j]);
                        }
                    }
                }
                _ => self.in_tune_since = None,
            }
        }
    }

    /// Afinação escolhida; aceita também o formato antigo do estado (só o nome, guitarra de 6 cordas).
    fn current_tuning(&self) -> &Tuning {
        let key = &self.state.tuning;
        let i = self
            .tunings
            .iter()
            .position(|t| &t.id() == key)
            .or_else(|| self.tunings.iter().position(|t| &t.name == key && t.group() == "Guitarra 6 cordas"))
            .unwrap_or(0);
        &self.tunings[i]
    }

    fn select_tuning(&mut self, id: String) {
        self.state.tuning = id;
        self.reset_strings();
        self.dirty = true;
    }

    fn save_tuning(&mut self, ed: &TuningEditor) -> Result<(), String> {
        let mut t = ed.tuning.clone();
        t.name = t.name.trim().to_string();
        t.notes = t.notes.trim().to_string();
        t.custom = true;
        if t.name.is_empty() {
            return Err("Dê um nome à afinação.".into());
        }
        let id = t.id();
        let original = ed.original.as_deref();
        if self.tunings.iter().any(|x| !x.custom && x.id() == id) {
            return Err("Já existe uma afinação de fábrica com esse nome para esse instrumento.".into());
        }
        if self.tunings.iter().any(|x| x.custom && x.id() == id && Some(id.as_str()) != original) {
            return Err("Você já tem uma afinação com esse nome para esse instrumento.".into());
        }
        let mut custom: Vec<Tuning> =
            self.tunings.iter().filter(|x| x.custom && Some(x.id().as_str()) != original).cloned().collect();
        custom.push(t);
        custom.sort_by_key(|x| x.id().to_lowercase());
        preset::save_custom_tunings(&custom).map_err(|e| format!("{e:#}"))?;
        self.tunings = all_tunings();
        self.select_tuning(id);
        Ok(())
    }

    fn delete_tuning(&mut self, id: &str) -> Result<(), String> {
        let custom: Vec<Tuning> = self.tunings.iter().filter(|x| x.custom && x.id() != id).cloned().collect();
        preset::save_custom_tunings(&custom).map_err(|e| format!("{e:#}"))?;
        self.tunings = all_tunings();
        if self.state.tuning == id {
            self.select_tuning(Tuning::default().id());
        }
        Ok(())
    }

    fn reset_strings(&mut self) {
        self.strings_done = vec![false; self.current_tuning().midi.len()];
        // padrão automático (descobre a corda); travar numa corda é só clicando nela
        self.string_lock = if self.auto_strings { None } else { Some(0) };
        self.tracker.set(None);
        self.string_current = None;
        self.in_tune_since = None;
    }

    fn sync(&mut self) {
        let eff = self.effective_settings();
        if self.sent != Some(eff) {
            if let Some(e) = &self.engine {
                if e.send_settings(eff) {
                    self.sent = Some(eff);
                }
            }
        }
        if self.state.preset.settings != self.last_seen {
            self.last_seen = self.state.preset.settings;
            self.dirty = true;
        }
        if self.dirty && self.last_save.elapsed() > Duration::from_secs(3) {
            self.state.save();
            self.dirty = false;
            self.last_save = Instant::now();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some((path, frames)) = &mut self.print {
            *frames += 1;
            ctx.request_repaint();
            if *frames == 30 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                if let Err(e) = save_bmp(path, &image) {
                    log::error!("print: {e}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.poll();

        if !ctx.wants_keyboard_input() {
            let (mute, tune, rec, noise) =
                ctx.input(|i| (i.key_pressed(Key::M), i.key_pressed(Key::T), i.key_pressed(Key::R), i.key_pressed(Key::N)));
            if noise {
                let g = &mut self.state.preset.settings.gate;
                let on = g.on || g.hum_filter;
                g.on = !on;
                g.hum_filter = !on;
            }
            if mute {
                self.state.preset.settings.mute = !self.state.preset.settings.mute;
            }
            if tune {
                self.tuning = !self.tuning;
            }
            if rec {
                self.toggle_recording();
            }
        }

        // topo, rodapé e rack usam a mesma margem lateral: tudo alinha na mesma coluna
        let side = ((ctx.screen_rect().width() - MAX_W) / 2.0).max(MIN_SIDE).floor();
        self.top_bar(ctx, side);
        self.bottom_bar(ctx, side);
        egui::CentralPanel::default()
            .frame(Frame::none().fill(theme::PANEL).inner_margin(Margin::symmetric(0.0, 12.0)))
            .show(ctx, |ui| {
                let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if self.print_bottom && self.print.as_ref().is_some_and(|(_, f)| *f == 10) {
                    scroll = scroll.vertical_scroll_offset(10_000.0);
                }
                scroll.show(ui, |ui| {
                    Frame::none().inner_margin(Margin::symmetric(side, 0.0)).show(ui, |ui| {
                        self.preset_rows(ui);
                        ui.add_space(12.0);
                        self.tuner_card(ui);
                        ui.add_space(16.0);
                        self.rack(ui);
                    });
                });
            });
        self.save_dialog(ctx);
        self.tuning_editor(ctx);
        self.audio_dialog(ctx);

        self.sync();
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_recording();
        self.state.save();
    }
}

impl App {
    fn top_bar(&mut self, ctx: &egui::Context, side: f32) {
        egui::TopBottomPanel::top("topo")
            .exact_height(86.0)
            .frame(Frame::none().fill(theme::BG).inner_margin(Margin::symmetric(side, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.vertical(|ui| {
                        ui.add_space(4.0);
                        ui.label(RichText::new("GuitarForge").size(25.0).strong().color(theme::AMBER));
                        ui.label(RichText::new("amp para o cabo Rocksmith").size(11.5).color(theme::DIM));
                    });
                    ui.add_space(18.0);
                    self.status_pill(ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.mute_button(ui);
                        ui.add(
                            Knob::new(&mut self.state.preset.settings.master, "Volume")
                                .size(42.0)
                                .default_value(0.6),
                        );
                        self.level_meters(ui);
                        ui.add_space(6.0);
                        self.rec_button(ui);
                    });
                });
            });
    }

    fn status_pill(&mut self, ui: &mut Ui) {
        let mut open_dialog = false;
        let mut retry = false;
        Frame::none()
            .fill(theme::CARD)
            .rounding(Rounding::same(20.0))
            .inner_margin(Margin::symmetric(14.0, 7.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    match &self.engine {
                        Some(e) => {
                            let rocksmith = audio::is_rocksmith(&e.info.input);
                            let led = widgets::led(ui, true, if rocksmith { theme::GREEN } else { theme::YELLOW });
                            if !rocksmith {
                                led.on_hover_text("O cabo Rocksmith não foi encontrado, usando outra entrada.");
                            }
                            ui.vertical(|ui| {
                                ui.label(RichText::new(short_name(&e.info.input, 32)).strong());
                                ui.label(
                                    RichText::new(format!(
                                        "{} · atraso do app ~{:.0} ms",
                                        khz(e.info.rate),
                                        e.latency_ms(&self.meters)
                                    ))
                                    .small()
                                    .color(theme::DIM),
                                )
                                .on_hover_text(format!("Saída: {}", e.info.output));
                            });
                        }
                        None => {
                            widgets::led(ui, true, theme::RED);
                            let err = self.engine_error.clone().unwrap_or_else(|| "parado".into());
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Sem áudio").strong().color(theme::RED));
                                ui.label(RichText::new(truncate(&err, 58)).small().color(theme::DIM)).on_hover_text(err);
                            });
                            retry = ui.button("Tentar de novo").clicked();
                        }
                    }
                    open_dialog = ui.button("Áudio…").on_hover_text("Escolher entrada, saída e latência").clicked();
                });
            });
        if retry {
            self.start_engine();
        }
        if open_dialog {
            self.open_audio_dialog();
        }
    }

    fn mute_button(&mut self, ui: &mut Ui) {
        let muted = self.state.preset.settings.mute || self.tuning;
        let (text, color) = if muted { ("MUDO", theme::RED) } else { ("SOM LIGADO", theme::GREEN) };
        let button = Button::new(RichText::new(text).strong().color(theme::TEXT))
            .fill(color.gamma_multiply(if muted { 0.45 } else { 0.2 }))
            .stroke(Stroke::new(1.0_f32, color))
            .rounding(8.0)
            .min_size(vec2(112.0, 38.0));
        if ui.add(button).on_hover_text("Liga e desliga o som (tecla M)").clicked() {
            if self.tuning {
                self.tuning = false;
            } else {
                self.state.preset.settings.mute = !self.state.preset.settings.mute;
            }
        }
    }

    fn rec_button(&mut self, ui: &mut Ui) {
        let recording = self.recording.as_ref().map(|r| r.started.elapsed().as_secs());
        let text = match recording {
            Some(s) => format!("     PARAR {:02}:{:02}", s / 60, s % 60),
            None => "     GRAVAR".to_string(),
        };
        let button = Button::new(RichText::new(text).strong())
            .fill(if recording.is_some() { theme::RED.gamma_multiply(0.5) } else { theme::CARD })
            .stroke(Stroke::new(1.0_f32, if recording.is_some() { theme::RED } else { theme::FAINT }))
            .rounding(8.0)
            .min_size(vec2(122.0, 38.0));
        let resp = ui
            .add_enabled(self.engine.is_some(), button)
            .on_hover_text("Grava o som processado e o sinal seco do cabo (DI) em Música\\GuitarForge (tecla R)");
        let blink = recording.is_none() || (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0;
        if blink {
            ui.painter().circle_filled(resp.rect.left_center() + vec2(18.0, 0.0), 5.5, theme::RED);
        }
        if resp.clicked() {
            self.toggle_recording();
        }
    }

    fn level_meters(&self, ui: &mut Ui) {
        let clip = Instant::now() < self.clip_until;
        ui.allocate_ui(vec2(230.0, 60.0), |ui| {
            ui.vertical(|ui| {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_sized([54.0, 14.0], Label::new(RichText::new("ENTRADA").small().color(theme::DIM)));
                    widgets::level_bar(ui, self.in_db, self.in_hold, 120.0)
                        .on_hover_text(format!("Pico do cabo: {:.1} dBFS", self.in_hold));
                    ui.label(RichText::new("CLIP").small().strong().color(if clip { theme::RED } else { theme::FAINT }));
                });
                ui.horizontal(|ui| {
                    ui.add_sized([54.0, 14.0], Label::new(RichText::new("SAÍDA").small().color(theme::DIM)));
                    widgets::level_bar(ui, self.out_db, -90.0, 120.0);
                });
            })
        });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context, side: f32) {
        egui::TopBottomPanel::bottom("diagnostico")
            .exact_height(36.0)
            .frame(Frame::none().fill(theme::BG).inner_margin(Margin::symmetric(side, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let m = &self.meters;
                    ui.label(RichText::new("DIAGNÓSTICO").small().strong().color(theme::DIM));
                    ui.add_space(6.0);
                    diag_item(ui, "Ruído de fundo", m.noise_floor_db.get(), "Menor nível dos últimos 5 s. Meça com a guitarra parada.");
                    diag_item(
                        ui,
                        &format!("Zumbido {:.0} Hz", m.mains_hz.get()),
                        m.hum_db.get(),
                        "Interferência da rede elétrica. Single coil sem blindagem fica por volta de -62 dBFS.",
                    );
                    let gate = &self.state.preset.settings.gate;
                    let open = m.gate_open.load(Relaxed);
                    widgets::led(ui, gate.on && open, theme::GREEN);
                    ui.label(
                        RichText::new(match (gate.on, open) {
                            (false, _) => "gate desligado",
                            (true, true) => "gate aberto",
                            (true, false) => "gate fechado",
                        })
                        .small()
                        .color(theme::DIM),
                    );
                    let underruns = m.underruns.load(Relaxed);
                    if underruns > 0 {
                        ui.add_space(8.0);
                        ui.label(RichText::new(format!("falhas de áudio: {underruns}")).small().color(theme::YELLOW))
                            .on_hover_text("Se ouvir estalos, aumente a latência em Áudio…");
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("Knobs: arraste, ou clique e use as setas · M mudo · T afinador · R gravar · N ruído").small().color(theme::FAINT));
                        ui.add_space(12.0);
                        if let Some(e) = &self.rec_error {
                            ui.label(RichText::new(truncate(e, 50)).small().color(theme::RED)).on_hover_text(e.clone());
                        } else if let Some(p) = &self.last_recording {
                            if ui.small_button("Abrir pasta").clicked() {
                                let _ = std::process::Command::new("explorer").arg(paths::recordings_dir()).spawn();
                            }
                            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            ui.label(RichText::new(format!("gravado: {name}")).small().color(theme::DIM));
                        }
                    });
                });
            });
    }

    fn preset_rows(&mut self, ui: &mut Ui) {
        let mut chosen = None;
        let mut save = false;
        let mut undo = false;
        let modified = !same_sound(&self.state.preset.settings, &self.baseline);
        ui.horizontal(|ui| {
            row_label(ui, "PRESET");
            let title = if modified { format!("{} (alterado)", self.state.preset.name) } else { self.state.preset.name.clone() };
            ComboBox::from_id_salt("preset").width(240.0).selected_text(title).show_ui(ui, |ui| {
                ui.label(RichText::new("De fábrica").small().color(theme::DIM));
                for p in &self.factory {
                    if ui.selectable_label(p.name == self.state.preset.name, &p.name).clicked() {
                        chosen = Some(p.clone());
                    }
                }
                if !self.user.is_empty() {
                    ui.separator();
                    ui.label(RichText::new("Meus presets").small().color(theme::DIM));
                    for p in &self.user {
                        if ui.selectable_label(p.name == self.state.preset.name, &p.name).clicked() {
                            chosen = Some(p.clone());
                        }
                    }
                }
            });
            save = ui.button("Salvar…").on_hover_text("Guarda o som atual como um preset seu").clicked();
            if modified {
                undo = ui.button("Desfazer").on_hover_text("Volta ao preset como estava").clicked();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let g = &mut self.state.preset.settings.gate;
                let on = g.on || g.hum_filter;
                let text = if on { "Supressão de ruído: ligada" } else { "Supressão de ruído: desligada" };
                let button = Button::new(RichText::new(text).color(if on { theme::GREEN } else { theme::DIM }))
                    .fill(if on { theme::GREEN.gamma_multiply(0.12) } else { theme::CARD_OFF })
                    .stroke(Stroke::new(1.0_f32, if on { theme::GREEN.gamma_multiply(0.7) } else { theme::FAINT }));
                let hint = "Corta o chiado entre as notas (gate) e o zumbido da rede elétrica. Vale para todos os presets (tecla N). \
                            Nota longa sendo cortada: aumente a Espera e o Fecha no cartão Entrada e Gate. \
                            O WAV (DI) da gravação sai sempre sem corte.";
                if ui.add(button).on_hover_text(hint).clicked() {
                    g.on = !on;
                    g.hum_filter = !on;
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            row_label(ui, "CADEIA");
            chain_strip(ui, &mut self.state.preset.settings);
        });
        if let Some(p) = chosen {
            self.apply_preset(p);
        }
        if undo {
            let keep = self.state.preset.settings;
            self.state.preset.settings = self.baseline;
            self.state.preset.settings.master = keep.master;
            self.state.preset.settings.mute = keep.mute;
            self.state.preset.settings.input_gain_db = keep.input_gain_db;
            self.state.preset.settings.gate = keep.gate;
        }
        if save {
            self.save_error = None;
            self.save_name = Some(if self.state.preset.factory { "Meu som".into() } else { self.state.preset.name.clone() });
        }
    }

    fn tuner_card(&mut self, ui: &mut Ui) {
        let hz = self.meters.pitch_hz.get();
        let note = (hz > 0.0).then(|| tuner::note_for(hz));
        let tuning_mode = self.tuning;
        let tuning = self.current_tuning().clone();
        let current_id = tuning.id();
        let notation = self.state.notation;
        let n = tuning.midi.len();
        let shown = if note.is_some() { self.string_current } else { self.string_lock };
        let target = shown.filter(|&i| i < n).map(|i| widgets::TunerTarget {
            string: tuning.string_number(i) as u8,
            midi: tuning.midi[i],
            cents: note.map(|_| tuner::cents_between(hz, tuner::midi_freq(tuning.midi[i]))),
            locked: self.string_lock.is_some(),
            other_string: match (self.string_lock, note) {
                (Some(l), Some(_)) => tuner::pick_string(hz, &tuning, None)
                    .filter(|&j| j != l && tuner::cents_between(hz, tuner::midi_freq(tuning.midi[j])).abs() < 60.0)
                    .map(|j| tuning.string_number(j) as u8),
                _ => None,
            },
        });
        let mut new_tuning = None;
        let mut new_notation = None;
        let mut lock = None;
        let mut restart = false;
        let mut open_editor = None;

        Frame::none()
            .fill(theme::CARD)
            .rounding(Rounding::same(12.0))
            .stroke(if tuning_mode { Stroke::new(1.5_f32, theme::GREEN) } else { Stroke::new(1.0_f32, theme::FAINT) })
            .inner_margin(Margin::symmetric(CARD_PAD, 10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("AFINADOR").small().strong().color(theme::DIM));
                    ui.add_space(8.0);
                    ComboBox::from_id_salt("afinacao")
                        .width(400.0)
                        .height(560.0)
                        .selected_text(format!("{} · {} ({})", tuning.name, tuning.group(), tuning.note_names(notation)))
                        .show_ui(ui, |ui| {
                            let mut group = String::new();
                            for t in self.tunings.iter().filter(|t| !t.custom) {
                                if t.group() != group {
                                    group = t.group();
                                    ui.label(RichText::new(&group).small().strong().color(theme::DIM));
                                }
                                let text = format!("   {}  ({})", t.name, t.note_names(notation));
                                if ui.selectable_label(t.id() == current_id, text).clicked() {
                                    new_tuning = Some(t.id());
                                }
                            }
                            if self.tunings.iter().any(|t| t.custom) {
                                ui.separator();
                                ui.label(RichText::new("Minhas afinações").small().strong().color(theme::DIM));
                                for t in self.tunings.iter().filter(|t| t.custom) {
                                    let text = format!("   {} · {} ({})", t.name, t.group(), t.note_names(notation));
                                    let resp = ui.selectable_label(t.id() == current_id, text);
                                    let resp = if t.notes.is_empty() { resp } else { resp.on_hover_text(&t.notes) };
                                    if resp.clicked() {
                                        new_tuning = Some(t.id());
                                    }
                                }
                            }
                        });
                    if ui.button("Nova…").on_hover_text("Cria uma afinação sua a partir desta").clicked() {
                        open_editor = Some(false);
                    }
                    if tuning.custom && ui.button("Editar…").clicked() {
                        open_editor = Some(true);
                    }
                    ui.add_space(8.0);
                    for nt in [tuner::Notation::Solfege, tuner::Notation::Letters] {
                        if ui.add(Button::new(nt.label()).selected(notation == nt)).on_hover_text("Nome das notas").clicked() {
                            new_notation = Some(nt);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if tuning_mode { "Sair do modo afinação" } else { "Modo afinação (silencia o som)" };
                        if ui.add(Button::new(label).selected(tuning_mode)).on_hover_text("Tecla T").clicked() {
                            self.tuning = !self.tuning;
                        }
                    });
                });
                if !tuning.notes.is_empty() {
                    ui.label(RichText::new(&tuning.notes).small().color(theme::DIM));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let auto = self.string_lock.is_none();
                    if ui
                        .add(Button::new("Automático").selected(auto).min_size(vec2(96.0, 28.0)))
                        .on_hover_text("Descobre sozinho qual corda você está tocando (padrão). Clicando numa corda, ele afina só ela e depois segue para a próxima.")
                        .clicked()
                    {
                        lock = Some(None);
                    }
                    ui.add_space(6.0);
                    let chip_w = if n > 7 { 62.0 } else { 72.0 };
                    for i in 0..n {
                        let done = self.strings_done.get(i).copied().unwrap_or(false);
                        let active = shown == Some(i);
                        let name = notation.name(tuning.midi[i].rem_euclid(12) as usize);
                        let text = RichText::new(format!("{}ª  {name}", tuning.string_number(i)))
                            .color(if done { theme::GREEN } else { theme::TEXT });
                        let stroke = if active {
                            Stroke::new(2.0_f32, theme::AMBER)
                        } else if done {
                            Stroke::new(1.0_f32, theme::GREEN)
                        } else {
                            Stroke::new(1.0_f32, theme::FAINT)
                        };
                        let b = Button::new(text)
                            .fill(if done { theme::GREEN.gamma_multiply(0.15) } else { theme::CARD_OFF })
                            .stroke(stroke)
                            .min_size(vec2(chip_w, 28.0));
                        let hint = format!("{}: afinar esta corda; ao ficar afinada, passa para a próxima", notation.midi(tuning.midi[i]));
                        if ui.add(b).on_hover_text(hint).clicked() {
                            lock = Some(Some(i));
                        }
                    }
                    let done = self.strings_done.iter().filter(|d| **d).count();
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!("{done} de {n} afinadas"))
                            .small()
                            .color(if done == n { theme::GREEN } else { theme::DIM }),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Recomeçar").on_hover_text("Marca todas as cordas como não afinadas").clicked() {
                            restart = true;
                        }
                    });
                });
                ui.add_space(4.0);
                widgets::tuner_view(ui, note, target, self.needle, notation);
            });

        if let Some(id) = new_tuning {
            self.select_tuning(id);
        }
        if let Some(nt) = new_notation {
            self.state.notation = nt;
            self.dirty = true;
        }
        if let Some(l) = lock {
            self.auto_strings = l.is_none();
            self.tracker.set(l);
            self.string_lock = l;
            self.string_current = l;
            self.in_tune_since = None;
        }
        if restart {
            self.reset_strings();
        }
        if let Some(edit) = open_editor {
            let mut t = tuning.clone();
            let original = if edit {
                Some(t.id())
            } else {
                t.name = format!("{} (minha)", t.name);
                t.notes.clear();
                None
            };
            t.custom = true;
            self.tuning_editor = Some(TuningEditor { original, tuning: t, error: None, confirm_delete: false });
        }
    }

    fn tuning_editor(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.tuning_editor.take() else { return };
        let notation = self.state.notation;
        let mut keep_open = true;
        let mut save = false;
        let mut delete = false;
        let title = if ed.original.is_some() { "Editar afinação" } else { "Nova afinação" };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("afinacao_dados").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Nome");
                    ui.add(egui::TextEdit::singleline(&mut ed.tuning.name).desired_width(280.0));
                    ui.end_row();

                    ui.label("Instrumento");
                    ui.horizontal(|ui| {
                        for inst in Instrument::ALL {
                            ui.selectable_value(&mut ed.tuning.instrument, inst, inst.label());
                        }
                    });
                    ui.end_row();

                    ui.label("Cordas");
                    ui.horizontal(|ui| {
                        let count = ed.tuning.midi.len();
                        if ui
                            .add_enabled(count > tuner::MIN_STRINGS, Button::new(" - "))
                            .on_hover_text("Tira a corda mais grave")
                            .clicked()
                        {
                            ed.tuning.midi.remove(0);
                        }
                        ui.label(RichText::new(count.to_string()).strong());
                        if ui
                            .add_enabled(count < tuner::MAX_STRINGS, Button::new(" + "))
                            .on_hover_text("Acrescenta uma corda mais grave, uma quarta abaixo")
                            .clicked()
                        {
                            let low = ed.tuning.midi[0];
                            ed.tuning.midi.insert(0, low - 5);
                        }
                    });
                    ui.end_row();

                    ui.label("Observação");
                    ui.add(
                        egui::TextEdit::singleline(&mut ed.tuning.notes)
                            .desired_width(280.0)
                            .hint_text("artista, música, qual instrumento"),
                    );
                    ui.end_row();
                });

                ui.add_space(6.0);
                ui.label(RichText::new("Nota de cada corda, da mais aguda para a mais grave:").small().color(theme::DIM));
                egui::Grid::new("afinacao_cordas").num_columns(5).spacing([10.0, 4.0]).show(ui, |ui| {
                    let count = ed.tuning.midi.len();
                    for i in (0..count).rev() {
                        ui.label(format!("{}ª corda", count - i));
                        if ui.small_button(" - ").on_hover_text("Meio tom abaixo").clicked() {
                            ed.tuning.midi[i] -= 1;
                        }
                        ed.tuning.midi[i] = ed.tuning.midi[i].clamp(12, 96);
                        ui.add_sized([60.0, 18.0], Label::new(RichText::new(notation.midi(ed.tuning.midi[i])).strong()));
                        if ui.small_button(" + ").on_hover_text("Meio tom acima").clicked() {
                            ed.tuning.midi[i] += 1;
                        }
                        ed.tuning.midi[i] = ed.tuning.midi[i].clamp(12, 96);
                        ui.label(
                            RichText::new(format!("{:.1} Hz", tuner::midi_freq(ed.tuning.midi[i])))
                                .small()
                                .color(theme::DIM),
                        );
                        ui.end_row();
                    }
                });

                if let Some(e) = &ed.error {
                    ui.colored_label(theme::RED, e);
                }
                ui.label(RichText::new(format!("Fica em {}", paths::tunings_file().display())).small().color(theme::DIM));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Salvar").clicked() {
                        save = true;
                    }
                    if ed.original.is_some() {
                        let label = if ed.confirm_delete { "Confirmar exclusão" } else { "Excluir" };
                        if ui.button(RichText::new(label).color(theme::RED)).clicked() {
                            if ed.confirm_delete {
                                delete = true;
                            } else {
                                ed.confirm_delete = true;
                            }
                        }
                    }
                    if ui.button("Cancelar").clicked() {
                        keep_open = false;
                    }
                });
            });

        if save {
            match self.save_tuning(&ed) {
                Ok(()) => keep_open = false,
                Err(e) => ed.error = Some(e),
            }
        }
        if delete {
            if let Some(id) = ed.original.clone() {
                match self.delete_tuning(&id) {
                    Ok(()) => keep_open = false,
                    Err(e) => ed.error = Some(e),
                }
            }
        }
        if keep_open {
            self.tuning_editor = Some(ed);
        }
    }

    fn rack(&mut self, ui: &mut Ui) {
        let ir_name = self.ir.as_ref().map(|ir| ir.name.clone());
        let ir_error = self.ir_error.clone();
        let gate_auto = Some(self.meters.gate_auto_db.get()).filter(|db| *db > -110.0);
        let s = &mut self.state.preset.settings;
        let total = ui.available_width();

        section_title(ui, "PEDAIS", "antes do amplificador, na ordem do sinal");
        for (i, row) in balanced_rows(total, &Pedal::ALL, Pedal::min_w).iter().enumerate() {
            if i > 0 {
                ui.add_space(GAP);
            }
            let mins: Vec<f32> = row.iter().map(|p| p.min_w()).collect();
            let w = card_widths(total, &mins);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for (&p, &w) in row.iter().zip(&w) {
                    pedal(ui, p, s, w, gate_auto);
                }
            });
        }

        ui.add_space(18.0);
        section_title(ui, "AMPLIFICADOR E GABINETE", "o coração do som");
        let w = card_widths(total, &[480.0, 280.0]);
        let mut action = CabAction::None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            amp_panel(ui, s, w[0]);
            action = cab_panel(ui, s, w[1], ir_name.as_deref(), ir_error.as_deref());
        });

        ui.add_space(18.0);
        section_title(ui, "DEPOIS DO AMP", "equalizador, eco e ambiência");
        let w = card_widths(total, &[400.0, 230.0, 150.0]);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            eq_panel(ui, s, w[0]);
            delay_panel(ui, s, w[1]);
            reverb_panel(ui, s, w[2]);
        });
        ui.add_space(12.0);

        match action {
            CabAction::Load => self.pick_ir(),
            CabAction::Remove => self.remove_ir(),
            CabAction::None => {}
        }
    }

    fn save_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.save_name.take() else { return };
        let mut keep_open = true;
        let mut do_save = false;
        let error = self.save_error.clone();
        egui::Window::new("Salvar preset")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Nome do preset:");
                let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(280.0));
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    do_save = true;
                }
                if let Some(e) = &error {
                    ui.colored_label(theme::RED, e);
                }
                ui.label(RichText::new(format!("Fica em {}", paths::presets_dir().display())).small().color(theme::DIM));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Salvar").clicked() {
                        do_save = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        keep_open = false;
                    }
                });
            });
        if do_save {
            match self.save_user_preset(name.trim()) {
                Ok(()) => keep_open = false,
                Err(e) => self.save_error = Some(e),
            }
        }
        if keep_open {
            self.save_name = Some(name);
        }
    }

    fn audio_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.audio_dialog.take() else { return };
        let mut keep_open = true;
        let mut apply = false;
        let mut refresh = false;
        egui::Window::new("Áudio")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("audio_grid").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                    ui.label("Entrada");
                    let auto_in = "Automático (cabo Rocksmith)";
                    ComboBox::from_id_salt("entrada")
                        .width(360.0)
                        .selected_text(d.input.clone().unwrap_or_else(|| auto_in.into()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.input, None, auto_in);
                            for n in &self.devices.inputs {
                                ui.selectable_value(&mut d.input, Some(n.clone()), n);
                            }
                        });
                    ui.end_row();

                    ui.label("Saída");
                    let auto_out = "Padrão do Windows";
                    ComboBox::from_id_salt("saida")
                        .width(360.0)
                        .selected_text(d.output.clone().unwrap_or_else(|| auto_out.into()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.output, None, auto_out);
                            for n in &self.devices.outputs {
                                ui.selectable_value(&mut d.output, Some(n.clone()), n);
                            }
                        });
                    ui.end_row();

                    ui.label("Latência");
                    ui.horizontal(|ui| {
                        for mode in LatencyMode::ALL {
                            ui.selectable_value(&mut d.latency, mode, mode.label());
                        }
                    });
                    ui.end_row();

                    ui.label("Sobreamostragem");
                    ui.horizontal(|ui| {
                        for o in Oversample::ALL {
                            ui.selectable_value(&mut d.oversample, o, o.label()).on_hover_text(
                                "O drive e o amp rodam a 2x ou 4x a taxa do cabo: some o chiado metálico da distorção forte. \
                                 Cada um soma uns 0,5 ms (2x) ou 0,7 ms (4x) de atraso e gasta mais processador.",
                            );
                        }
                    });
                    ui.end_row();

                    ui.label("Modo exclusivo");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut d.exclusive.input, "Entrada").on_hover_text(
                            "O GuitarForge fica dono do cabo e lê blocos de 3 ms em vez de 10 ms. Sem efeito colateral: o cabo só serve a ele.",
                        );
                        ui.checkbox(&mut d.exclusive.output, "Saída").on_hover_text(
                            "Uns 10 ms a menos, mas enquanto o GuitarForge estiver aberto os outros programas ficam sem som nessa saída. \
                             Fone sem fio USB pode não aguentar: o app confere a taxa logo depois de abrir e, se o fone errar, volta sozinho \
                             para o compartilhado.",
                        );
                    });
                    ui.end_row();
                });

                ui.add_space(6.0);
                match &self.engine {
                    Some(e) => {
                        let mode = |ex: bool| if ex { "exclusivo" } else { "compartilhado" };
                        let windows = if e.info.exclusive.input && e.info.exclusive.output {
                            "o driver soma só uns 2 ms"
                        } else {
                            "o Windows soma uns 10 a 20 ms no lado compartilhado"
                        };
                        ui.label(
                            RichText::new(format!(
                                "Em uso: {} ({}, {}) › {} ({}, {}). Atraso do app ~{:.0} ms; {windows}.",
                                e.info.input,
                                khz(e.info.rate),
                                mode(e.info.exclusive.input),
                                e.info.output,
                                khz(e.info.out_rate),
                                mode(e.info.exclusive.output),
                                e.latency_ms(&self.meters)
                            ))
                            .small()
                            .color(theme::DIM),
                        );
                        if let Some(n) = &e.info.notice {
                            ui.label(
                                RichText::new(format!("Exclusivo não abriu ({n}); esse lado ficou no compartilhado."))
                                    .small()
                                    .color(theme::YELLOW),
                            );
                        }
                    }
                    None => {
                        if let Some(err) = &self.engine_error {
                            ui.colored_label(theme::RED, err);
                        }
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    apply = ui.button("Aplicar e reiniciar o áudio").clicked();
                    refresh = ui.button("Atualizar lista").clicked();
                    if ui.button("Fechar").clicked() {
                        keep_open = false;
                    }
                });
            });
        if refresh {
            self.devices = audio::list_devices();
        }
        if apply {
            self.state.input_device = d.input.clone();
            self.state.output_device = d.output.clone();
            self.state.latency = d.latency;
            self.state.exclusive = d.exclusive;
            self.state.oversample = d.oversample;
            self.dirty = true;
            self.start_engine();
        }
        if keep_open {
            self.audio_dialog = Some(d);
        }
    }
}

fn all_tunings() -> Vec<Tuning> {
    let mut list = tuner::builtin();
    list.extend(preset::custom_tunings());
    list
}

fn same_sound(a: &Settings, b: &Settings) -> bool {
    let mut x = *a;
    x.master = b.master;
    x.mute = b.mute;
    x.input_gain_db = b.input_gain_db;
    x.gate = b.gate;
    x == *b
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

/// "Microfone (Rocksmith USB Guitar Adapter)" vira "Rocksmith USB Guitar Adapter".
fn short_name(name: &str, max: usize) -> String {
    let inner = match (name.find('('), name.rfind(')')) {
        (Some(a), Some(b)) if b > a + 1 => &name[a + 1..b],
        _ => name,
    };
    truncate(inner, max)
}

fn khz(rate: u32) -> String {
    if rate % 1000 == 0 { format!("{} kHz", rate / 1000) } else { format!("{:.1} kHz", rate as f32 / 1000.0) }
}

fn drive_color(kind: DriveKind) -> Color32 {
    match kind {
        DriveKind::Overdrive => theme::OVERDRIVE,
        DriveKind::Distortion => theme::DISTORTION,
        DriveKind::Fuzz => theme::FUZZ,
    }
}

fn diag_item(ui: &mut Ui, label: &str, db: f32, help: &str) {
    let (word, color) = if db <= -110.0 {
        ("sem sinal", theme::FAINT)
    } else if db < -72.0 {
        ("baixo", theme::GREEN)
    } else if db < -55.0 {
        ("moderado", theme::YELLOW)
    } else {
        ("alto", theme::RED)
    };
    let value = if db <= -110.0 { "--".to_string() } else { format!("{db:.0} dBFS") };
    ui.label(RichText::new(format!("{label}: {value}")).small()).on_hover_text(help);
    ui.label(RichText::new(word).small().color(color));
    ui.add_space(8.0);
}

fn row_label(ui: &mut Ui, text: &str) {
    ui.allocate_ui_with_layout(vec2(LABEL_W, 22.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(LABEL_W);
        ui.label(RichText::new(text).small().strong().color(theme::DIM));
    });
}

fn section_title(ui: &mut Ui, title: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).small().strong().color(theme::DIM));
        ui.label(RichText::new(hint).small().color(theme::FAINT));
    });
    ui.add_space(4.0);
}

fn chain_strip(ui: &mut Ui, s: &mut Settings) {
    let arrow = |ui: &mut Ui| {
        ui.label(RichText::new("›").color(theme::FAINT));
    };
    let fixed = |ui: &mut Ui, label: &str| {
        ui.label(RichText::new(label).size(12.5).color(theme::DIM));
    };
    fixed(ui, "Cabo");
    let drive = drive_color(s.drive.kind);
    let stages: [(&str, &mut bool, Color32); 15] = [
        ("Gate", &mut s.gate.on, theme::GATE),
        ("Comp", &mut s.comp.on, theme::COMP),
        ("Wah", &mut s.wah.on, theme::WAH),
        ("Oitava", &mut s.octave.on, theme::OCTAVE),
        ("Boost", &mut s.boost.on, theme::BOOST),
        ("Drive", &mut s.drive.on, drive),
        ("Chorus", &mut s.chorus.on, theme::CHORUS),
        ("Phaser", &mut s.phaser.on, theme::PHASER),
        ("Flanger", &mut s.flanger.on, theme::FLANGER),
        ("Tremolo", &mut s.tremolo.on, theme::TREMOLO),
        ("Amp", &mut s.amp.on, theme::AMBER),
        ("Gabinete", &mut s.cab.on, theme::CAB),
        ("EQ", &mut s.eq.on, theme::EQ),
        ("Delay", &mut s.delay.on, theme::DELAY),
        ("Reverb", &mut s.reverb.on, theme::REVERB),
    ];
    for (label, on, color) in stages {
        arrow(ui);
        let button = Button::new(RichText::new(label).size(12.5).color(if *on { color } else { theme::DIM }))
            .fill(if *on { color.gamma_multiply(0.14) } else { theme::CARD_OFF })
            .stroke(Stroke::new(1.0_f32, if *on { color.gamma_multiply(0.8) } else { theme::FAINT }))
            .rounding(12.0);
        if ui.add(button).on_hover_text(if *on { "Ligado: clique para desligar" } else { "Desligado: clique para ligar" }).clicked() {
            *on = !*on;
        }
    }
    arrow(ui);
    fixed(ui, "Saída");
}

/// Largura interna de cada cartão de uma fileira: as mínimas crescem na mesma proporção até a fileira
/// ocupar a coluna inteira, então todas as fileiras terminam na mesma borda.
fn card_widths(total: f32, mins: &[f32]) -> Vec<f32> {
    let n = mins.len() as f32;
    let fixed = GAP * (n - 1.0) + 2.0 * CARD_PAD * n + 1.0;
    let sum: f32 = mins.iter().sum();
    let scale = ((total - fixed) / sum).max(1.0);
    mins.iter().map(|m| (m * scale).floor()).collect()
}

/// Reparte os itens em fileiras de tamanho parecido, sem mudar a ordem, com o menor número de fileiras em que
/// todas cabem na largura (cada cartão soma o recuo dos dois lados e o vão até o próximo).
fn balanced_rows<T: Copy>(total: f32, items: &[T], min_w: impl Fn(T) -> f32) -> Vec<Vec<T>> {
    let fits = |row: &[T]| {
        let n = row.len() as f32;
        row.iter().map(|&p| min_w(p)).sum::<f32>() + GAP * (n - 1.0) + 2.0 * CARD_PAD * n + 1.0 <= total
    };
    for rows in 1..=items.len() {
        let (base, extra) = (items.len() / rows, items.len() % rows);
        let mut out = Vec::with_capacity(rows);
        let mut start = 0;
        for r in 0..rows {
            let n = base + usize::from(r < extra);
            out.push(items[start..start + n].to_vec());
            start += n;
        }
        if out.iter().all(|r| fits(r)) {
            return out;
        }
    }
    items.iter().map(|&p| vec![p]).collect()
}

/// Pedais antes do amplificador, na ordem do sinal.
#[derive(Clone, Copy)]
enum Pedal {
    Input,
    Comp,
    Wah,
    Octave,
    Boost,
    Drive,
    Chorus,
    Phaser,
    Flanger,
    Tremolo,
}

impl Pedal {
    const ALL: [Pedal; 10] = [
        Self::Input,
        Self::Comp,
        Self::Wah,
        Self::Octave,
        Self::Boost,
        Self::Drive,
        Self::Chorus,
        Self::Phaser,
        Self::Flanger,
        Self::Tremolo,
    ];

    fn min_w(self) -> f32 {
        match self {
            Self::Input => 380.0,
            Self::Comp | Self::Boost => 150.0,
            _ => 230.0,
        }
    }
}

fn pedal(ui: &mut Ui, p: Pedal, s: &mut Settings, w: f32, gate_auto: Option<f32>) {
    match p {
        Pedal::Input => pedal_input(ui, s, w, gate_auto),
        Pedal::Comp => pedal_comp(ui, s, w),
        Pedal::Wah => pedal_wah(ui, s, w),
        Pedal::Octave => pedal_octave(ui, s, w),
        Pedal::Boost => pedal_boost(ui, s, w),
        Pedal::Drive => pedal_drive(ui, s, w),
        Pedal::Chorus => pedal_chorus(ui, s, w),
        Pedal::Phaser => pedal_phaser(ui, s, w),
        Pedal::Flanger => pedal_flanger(ui, s, w),
        Pedal::Tremolo => pedal_tremolo(ui, s, w),
    }
}

/// `n` itens de largura `item_w` espalhados por igual na largura disponível (folga limitada) e centralizados.
fn even_row(ui: &mut Ui, n: usize, item_w: f32, add: impl FnOnce(&mut Ui)) {
    let avail = ui.available_width();
    let gap = ((avail - n as f32 * item_w) / (n as f32 + 1.0)).clamp(6.0, 36.0);
    let used = n as f32 * item_w + (n as f32 - 1.0) * gap;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.add_space(((avail - used) / 2.0).max(0.0));
        add(ui);
    });
}

/// Moldura de pedal: título, dica, corpo de altura fixa e a chave de pisar centralizada embaixo.
#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut Ui,
    title: &str,
    hint: &str,
    accent: Color32,
    on: &mut bool,
    inner_w: f32,
    body_h: f32,
    body: impl FnOnce(&mut Ui),
) {
    let active = *on;
    Frame::none()
        .fill(if active { theme::CARD } else { theme::CARD_OFF })
        .stroke(if active { Stroke::new(1.5_f32, accent.gamma_multiply(0.7)) } else { Stroke::new(1.0_f32, theme::FAINT) })
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::same(CARD_PAD))
        .show(ui, |ui| {
            ui.set_width(inner_w);
            ui.vertical(|ui| {
                ui.label(RichText::new(title).strong().size(15.0).color(if active { accent } else { theme::DIM }));
                ui.add(Label::new(RichText::new(hint).small().color(theme::DIM)).truncate());
                ui.add_space(10.0);
                ui.allocate_ui_with_layout(vec2(inner_w, body_h), Layout::top_down(Align::Min), |ui| {
                    ui.set_min_size(vec2(inner_w, body_h));
                    if !active {
                        ui.multiply_opacity(0.5);
                    }
                    body(ui);
                });
                ui.add_space(4.0);
                ui.vertical_centered(|ui| {
                    widgets::stomp(ui, on);
                });
            });
        });
}

const PEDAL_BODY: f32 = 132.0;

fn pedal_input(ui: &mut Ui, s: &mut Settings, w: f32, gate_auto: Option<f32>) {
    card(ui, "Entrada e Gate", "Ganho e corte de chiado", theme::GATE, &mut s.gate.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 5, KNOB_W, |ui| {
            ui.add(Knob::new(&mut s.input_gain_db, "Nível").range(-12.0, 24.0, 0.0).step(0.5).bipolar().format(fmt_db).accent(theme::GATE))
                .on_hover_text("Ganho do cabo antes de tudo. Vale para todos os presets.");
            if s.gate.auto {
                // no automático o knob só mostra o limiar escolhido pelo ruído medido
                let mut shown = gate_auto.unwrap_or(s.gate.threshold_db);
                ui.add_enabled(false, Knob::new(&mut shown, "Limiar").range(-80.0, -20.0, -52.0).format(fmt_db).accent(theme::GATE))
                    .on_disabled_hover_text(match gate_auto {
                        Some(_) => "Automático: 10 dB acima do ruído mais baixo dos últimos 5 s (nunca acima de -40 dB).",
                        None => "Automático: medindo o ruído de fundo…",
                    });
            } else {
                ui.add(Knob::new(&mut s.gate.threshold_db, "Limiar").range(-80.0, -20.0, -52.0).step(0.5).format(fmt_db).accent(theme::GATE))
                    .on_hover_text("Energia abaixo da qual o gate corta o som.");
            }
            ui.add(Knob::new(&mut s.gate.hold_ms, "Espera").range(0.0, 2000.0, 50.0).step(10.0).format(fmt_ms).accent(theme::GATE))
                .on_hover_text("Quanto o gate espera aberto depois que a nota fica fraca. Aumente se notas longas forem cortadas.");
            ui.add(Knob::new(&mut s.gate.release_ms, "Fecha").range(10.0, 2000.0, 80.0).step(10.0).format(fmt_ms).accent(theme::GATE))
                .on_hover_text("Quanto tempo o som leva para sumir quando o gate fecha. Mais longo = fim de nota natural.");
            ui.add(Knob::new(&mut s.gate.range_db, "Redução").range(10.0, 90.0, 90.0).step(1.0).format(fmt_range).accent(theme::GATE))
                .on_hover_text("Quanto o gate abaixa ao fechar. \"total\" = mudo; menos deixa um fundo e soa menos cortado.");
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.gate.auto, RichText::new("Automático").small()).on_hover_text(
                "Ajusta o limiar sozinho pelo ruído de fundo do seu cabo e guitarra, e acompanha se ele mudar.",
            );
            ui.checkbox(&mut s.gate.hum_filter, RichText::new("Tirar zumbido").small()).on_hover_text(
                "Filtro do zumbido da rede elétrica (60 Hz e harmônicos; 50 Hz se for a rede do lugar) que os captadores single coil pegam. Fica antes do gate e não encosta nas notas.",
            );
        });
    });
}

fn pedal_comp(ui: &mut Ui, s: &mut Settings, w: f32) {
    card(ui, "Compressor", "Segura as notas", theme::COMP, &mut s.comp.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 2, KNOB_W, |ui| {
            ui.add(Knob::new(&mut s.comp.sustain, "Sustain").accent(theme::COMP));
            ui.add(Knob::new(&mut s.comp.level, "Volume").accent(theme::COMP).bipolar());
        });
    });
}

fn pedal_drive(ui: &mut Ui, s: &mut Settings, w: f32) {
    let accent = drive_color(s.drive.kind);
    card(ui, "Drive", "Overdrive, distorção ou fuzz", accent, &mut s.drive.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 3, KNOB_W, |ui| {
            ui.add(Knob::new(&mut s.drive.gain, "Ganho").accent(accent).default_value(0.4));
            ui.add(Knob::new(&mut s.drive.tone, "Tom").accent(accent));
            ui.add(Knob::new(&mut s.drive.level, "Volume").accent(accent).bipolar());
        });
        ui.add_space(2.0);
        even_row(ui, 3, KNOB_W, |ui| {
            ui.spacing_mut().button_padding = vec2(4.0, 2.0);
            for kind in DriveKind::ALL {
                let b = Button::new(RichText::new(kind.label()).size(11.5)).selected(s.drive.kind == kind);
                if ui.add_sized([KNOB_W, 24.0], b).clicked() {
                    s.drive.kind = kind;
                }
            }
        });
    });
}

fn pedal_chorus(ui: &mut Ui, s: &mut Settings, w: f32) {
    card(ui, "Chorus", "Som largo e brilhante", theme::CHORUS, &mut s.chorus.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 3, KNOB_W, |ui| {
            ui.add(Knob::new(&mut s.chorus.rate, "Velocidade").accent(theme::CHORUS).default_value(0.3));
            ui.add(Knob::new(&mut s.chorus.depth, "Profund.").accent(theme::CHORUS));
            ui.add(Knob::new(&mut s.chorus.mix, "Mix").accent(theme::CHORUS));
        });
    });
}

fn pedal_wah(ui: &mut Ui, s: &mut Settings, w: f32) {
    let c = &mut s.wah;
    card(ui, "Auto-wah", "A palhetada abre o filtro", theme::WAH, &mut c.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 3, KNOB_W, |ui| {
            ui.add(Knob::new(&mut c.sens, "Sensib.").accent(theme::WAH))
                .on_hover_text("Quanto a força da palhetada abre o filtro. Alto: abre até tocando de leve.");
            ui.add(Knob::new(&mut c.resonance, "Resson.").accent(theme::WAH))
                .on_hover_text("Alto: estreito e cantado. Baixo: largo e suave.");
            ui.add(Knob::new(&mut c.range, "Faixa").default_value(0.6).accent(theme::WAH))
                .on_hover_text("Até onde o filtro abre, de 1 a 3,5 kHz.");
        });
    });
}

fn pedal_octave(ui: &mut Ui, s: &mut Settings, w: f32) {
    let c = &mut s.octave;
    card(ui, "Oitava", "Uma nota por vez", theme::OCTAVE, &mut c.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 3, KNOB_W, |ui| {
            ui.add(Knob::new(&mut c.sub, "Abaixo").default_value(0.6).accent(theme::OCTAVE))
                .on_hover_text("Uma oitava abaixo, como um baixo junto. Em acorde ele se perde (nos pedais também).");
            ui.add(Knob::new(&mut c.up, "Acima").default_value(0.0).accent(theme::OCTAVE))
                .on_hover_text("Uma oitava acima, áspero, de solo psicodélico. Soa melhor com o drive depois.");
            ui.add(Knob::new(&mut c.dry, "Direto").default_value(0.7).accent(theme::OCTAVE)).on_hover_text("O som original.");
        });
    });
}

fn pedal_boost(ui: &mut Ui, s: &mut Settings, w: f32) {
    let c = &mut s.boost;
    card(ui, "Boost", "Empurra o drive e o amp", theme::BOOST, &mut c.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 2, KNOB_W, |ui| {
            ui.add(Knob::new(&mut c.level_db, "Ganho").range(0.0, 20.0, 6.0).step(0.5).format(fmt_db).accent(theme::BOOST))
                .on_hover_text("Volume a mais antes do drive e do amp: satura mais e destaca o solo.");
            ui.add(Knob::new(&mut c.bright, "Brilho").default_value(0.3).accent(theme::BOOST))
                .on_hover_text("Realça os agudos, como os treble boosters dos anos 60.");
        });
    });
}

fn pedal_phaser(ui: &mut Ui, s: &mut Settings, w: f32) {
    let c = &mut s.phaser;
    card(ui, "Phaser", "Redemoinho que sobe e desce", theme::PHASER, &mut c.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 3, KNOB_W, |ui| {
            ui.add(Knob::new(&mut c.rate, "Velocidade").default_value(0.3).accent(theme::PHASER));
            ui.add(Knob::new(&mut c.depth, "Profund.").default_value(0.7).accent(theme::PHASER));
            ui.add(Knob::new(&mut c.feedback, "Realim.").default_value(0.4).accent(theme::PHASER))
                .on_hover_text("Acentua o efeito, deixando o som mais vocal.");
        });
    });
}

fn pedal_flanger(ui: &mut Ui, s: &mut Settings, w: f32) {
    let c = &mut s.flanger;
    card(ui, "Flanger", "Jato que passa", theme::FLANGER, &mut c.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 3, KNOB_W, |ui| {
            ui.add(Knob::new(&mut c.rate, "Velocidade").default_value(0.2).accent(theme::FLANGER));
            ui.add(Knob::new(&mut c.depth, "Profund.").default_value(0.6).accent(theme::FLANGER));
            ui.add(Knob::new(&mut c.feedback, "Realim.").default_value(0.5).accent(theme::FLANGER))
                .on_hover_text("Alto: metálico, de avião. Baixo: suave, quase um chorus.");
        });
    });
}

fn pedal_tremolo(ui: &mut Ui, s: &mut Settings, w: f32) {
    let c = &mut s.tremolo;
    card(ui, "Tremolo", "Volume que pulsa", theme::TREMOLO, &mut c.on, w, PEDAL_BODY, |ui| {
        even_row(ui, 3, KNOB_W, |ui| {
            ui.add(Knob::new(&mut c.rate, "Velocidade").default_value(0.4).accent(theme::TREMOLO)).on_hover_text("De 1 a 12 pulsos por segundo.");
            ui.add(Knob::new(&mut c.depth, "Profund.").accent(theme::TREMOLO));
            ui.add(Knob::new(&mut c.shape, "Forma").default_value(0.0).accent(theme::TREMOLO))
                .on_hover_text("Zero: onda suave, de amp antigo. No máximo: picotado, quase liga e desliga.");
        });
    });
}

const AMP_BODY: f32 = 146.0;

fn amp_panel(ui: &mut Ui, s: &mut Settings, w: f32) {
    let a = &mut s.amp;
    card(ui, "Amplificador", "Escolha o canal e ajuste o timbre", theme::AMBER, &mut a.on, w, AMP_BODY, |ui| {
        even_row(ui, 3, 120.0, |ui| {
            for ch in AmpChannel::ALL {
                let b = Button::new(RichText::new(ch.label()).strong()).selected(a.channel == ch);
                let hint = match ch {
                    AmpChannel::Clean => "Limpo, com folga: para acordes e dedilhado",
                    AmpChannel::Crunch => "Saturado na medida: rock clássico, blues",
                    AmpChannel::Lead => "Muito ganho: solos, hard rock, metal",
                };
                if ui.add_sized([120.0, 30.0], b).on_hover_text(hint).clicked() {
                    a.channel = ch;
                }
            }
        });
        ui.add_space(10.0);
        even_row(ui, 6, KNOB_W, |ui| {
            ui.add(Knob::new(&mut a.gain, "Ganho").size(56.0).default_value(0.4));
            ui.add(Knob::new(&mut a.bass, "Graves").size(56.0).bipolar());
            ui.add(Knob::new(&mut a.mid, "Médios").size(56.0).bipolar());
            ui.add(Knob::new(&mut a.treble, "Agudos").size(56.0).bipolar());
            ui.add(Knob::new(&mut a.presence, "Presença").size(56.0).default_value(0.4));
            ui.add(Knob::new(&mut a.volume, "Volume").size(56.0).default_value(0.6));
        });
    });
}

fn cab_panel(ui: &mut Ui, s: &mut Settings, w: f32, ir_name: Option<&str>, ir_error: Option<&str>) -> CabAction {
    let mut action = CabAction::None;
    let c = &mut s.cab;
    card(ui, "Gabinete", "A caixa e o alto-falante do amp", theme::CAB, &mut c.on, w, AMP_BODY, |ui| {
        ui.add_enabled_ui(!(c.use_ir && ir_name.is_some()), |ui| {
            for kind in CabKind::ALL {
                let b = Button::new(kind.label()).selected(c.kind == kind);
                if ui.add_sized([w, 24.0], b).clicked() {
                    c.kind = kind;
                }
            }
        });
        ui.add_space(4.0);
        match (ir_error, ir_name) {
            (Some(e), _) => {
                ui.label(RichText::new(truncate(e, 44)).small().color(theme::RED)).on_hover_text(e);
            }
            (None, Some(name)) => {
                ui.checkbox(&mut c.use_ir, format!("Usar IR: {}", truncate(name, 30)));
            }
            (None, None) => {
                ui.label(RichText::new("Sem IR: usando o gabinete interno").small().color(theme::DIM));
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Carregar IR (.wav)…").on_hover_text("Resposta ao impulso de um gabinete real, em WAV").clicked() {
                action = CabAction::Load;
            }
            if ir_name.is_some() && ui.button("Remover").clicked() {
                action = CabAction::Remove;
            }
        });
    });
    action
}

const FX_BODY: f32 = 122.0;

fn eq_panel(ui: &mut Ui, s: &mut Settings, w: f32) {
    let e = &mut s.eq;
    card(ui, "Equalizador", "5 faixas, ±12 dB, e cortes", theme::EQ, &mut e.on, w, FX_BODY, |ui| {
        ui.columns(2, |cols| {
            cols[0].spacing_mut().item_spacing.x = 4.0;
            even_row(&mut cols[0], 5, widgets::FADER_W, |ui| {
                for (band, freq) in e.bands_db.iter_mut().zip(EQ_FREQS) {
                    ui.add(widgets::Fader::new(band, 12.0, freq_label(freq), theme::EQ));
                }
            });
            even_row(&mut cols[1], 2, KNOB_W, |ui| {
                ui.add(Knob::new(&mut e.low_cut_hz, "Corta grave").range(20.0, 400.0, 20.0).step(5.0).format(fmt_low_cut).accent(theme::EQ))
                    .on_hover_text("Tira o grave abaixo daqui: aperta o som com muito ganho e limpa o embolado.");
                ui.add(
                    Knob::new(&mut e.high_cut_hz, "Corta agudo").range(2000.0, 20000.0, 20000.0).step(100.0).format(fmt_high_cut).accent(theme::EQ),
                )
                .on_hover_text("Tira o agudo acima daqui: some o chiado e o \"zumbido de abelha\" da distorção.");
            });
        });
    });
}

fn freq_label(f: f32) -> String {
    if f >= 1000.0 {
        let k = f / 1000.0;
        if k.fract() == 0.0 { format!("{k:.0}k") } else { format!("{k:.1}k").replace('.', ",") }
    } else {
        format!("{f:.0}")
    }
}

fn fmt_range(v: f32) -> String {
    if v >= 90.0 { "total".into() } else { format!("-{v:.0} dB") }
}

fn fmt_low_cut(v: f32) -> String {
    if v <= 20.5 { "desl.".into() } else { format!("{v:.0} Hz") }
}

fn fmt_high_cut(v: f32) -> String {
    if v >= 19500.0 { "desl.".into() } else { freq_label((v / 100.0).round() * 100.0) }
}

fn fmt_pct(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

fn delay_panel(ui: &mut Ui, s: &mut Settings, w: f32) {
    let d = &mut s.delay;
    card(ui, "Delay", "Eco que repete a nota", theme::DELAY, &mut d.on, w, FX_BODY, |ui| {
        even_row(ui, 3, KNOB_W, |ui| {
            ui.add(Knob::new(&mut d.time_ms, "Tempo").range(40.0, 1200.0, 380.0).step(5.0).format(fmt_ms).accent(theme::DELAY));
            ui.add(Knob::new(&mut d.feedback, "Repetições").range(0.0, 0.9, 0.35).step(0.01).format(fmt_pct).accent(theme::DELAY));
            ui.add(Knob::new(&mut d.mix, "Mix").default_value(0.3).accent(theme::DELAY));
        });
    });
}

fn reverb_panel(ui: &mut Ui, s: &mut Settings, w: f32) {
    let r = &mut s.reverb;
    card(ui, "Reverb", "Ambiência de sala", theme::REVERB, &mut r.on, w, FX_BODY, |ui| {
        even_row(ui, 2, KNOB_W, |ui| {
            ui.add(Knob::new(&mut r.size, "Sala").accent(theme::REVERB));
            ui.add(Knob::new(&mut r.mix, "Mix").default_value(0.2).accent(theme::REVERB));
        });
    });
}
