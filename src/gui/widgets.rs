use std::f32::consts::PI;

use egui::{Align2, Color32, FontId, Key, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, Widget, pos2, vec2};

use super::theme;
use crate::dsp::tuner::{Notation, Note};

pub const KNOB_W: f32 = 70.0;

/// Knob giratório.
/// Mouse: arrastar para cima ou para a direita aumenta (Shift = fino); duplo clique volta ao padrão.
/// Clique seleciona o knob (anel colorido). Selecionado: setas ajustam passo a passo (Shift = fino),
/// PageUp/PageDown dão passos grandes, Home/End vão aos extremos, Delete volta ao padrão, a roda do mouse
/// anda de passo em passo e Esc solta. Tab passa para o próximo knob.
/// A roda só mexe no knob selecionado: com o cursor só passando por cima, ela rola a página.
pub struct Knob<'a> {
    value: &'a mut f32,
    min: f32,
    max: f32,
    default: f32,
    step: Option<f32>,
    label: &'a str,
    diameter: f32,
    accent: Color32,
    bipolar: bool,
    format: fn(f32) -> String,
}

impl<'a> Knob<'a> {
    pub fn new(value: &'a mut f32, label: &'a str) -> Self {
        Self {
            value,
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: None,
            label,
            diameter: 50.0,
            accent: theme::AMBER,
            bipolar: false,
            format: fmt_ten,
        }
    }

    pub fn range(mut self, min: f32, max: f32, default: f32) -> Self {
        self.min = min;
        self.max = max;
        self.default = default;
        self
    }

    pub fn default_value(mut self, default: f32) -> Self {
        self.default = default;
        self
    }

    /// Passo das setas e da roda (padrão: 1% da faixa).
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    pub fn accent(mut self, accent: Color32) -> Self {
        self.accent = accent;
        self
    }

    pub fn size(mut self, diameter: f32) -> Self {
        self.diameter = diameter;
        self
    }

    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }

    pub fn format(mut self, format: fn(f32) -> String) -> Self {
        self.format = format;
        self
    }
}

fn trim_zero(s: String) -> String {
    match s.strip_suffix('0') {
        Some(t) if t.contains('.') && !t.ends_with('.') => t.to_string(),
        _ => s,
    }
}

/// Escala de 0 a 10 como nos amps; mostra a 2ª casa só quando o ajuste fino a usa.
pub fn fmt_ten(v: f32) -> String {
    trim_zero(format!("{:.2}", v * 10.0))
}

pub fn fmt_db(v: f32) -> String {
    let s = format!("{v:+.1}");
    format!("{} dB", s.strip_suffix(".0").unwrap_or(&s))
}

pub fn fmt_ms(v: f32) -> String {
    format!("{v:.0} ms")
}

// o egui nativo conta 40 pontos por linha (clique) da roda
const WHEEL_NOTCH: f32 = 40.0;

impl Widget for Knob<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let d = self.diameter;
        let width = d.max(KNOB_W);
        let (rect, mut resp) = ui.allocate_exact_size(vec2(width, d + 32.0), Sense::click_and_drag());
        let span = self.max - self.min;
        let step = self.step.unwrap_or(span / 100.0);
        let mut v = *self.value;

        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let delta = resp.drag_delta();
            v += (delta.x - delta.y) / if fine { 1500.0 } else { 300.0 } * span;
        }
        if resp.double_clicked() {
            v = self.default;
        }

        let keys = selectable_input(ui, &resp);
        let focused = keys.is_some();
        if let Some(k) = keys {
            v += k.steps * step;
            if k.reset {
                v = self.default;
            }
            if k.to_min {
                v = self.min;
            }
            if k.to_max {
                v = self.max;
            }
        }

        v = v.clamp(self.min, self.max);
        if v != *self.value {
            *self.value = v;
            resp.mark_changed();
        }

        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            let c = pos2(rect.center().x, rect.top() + d / 2.0);
            let r = d / 2.0 - 3.0;
            let t = (v - self.min) / span;
            let angle = |t: f32| 0.75 * PI + 1.5 * PI * t;
            let active = focused || resp.dragged() || resp.hovered();

            if focused {
                p.circle_filled(c, r + 2.0, self.accent.gamma_multiply(0.12));
                p.circle_stroke(c, r + 2.5, Stroke::new(1.5_f32, self.accent));
            }
            arc(p, c, r, angle(0.0), angle(1.0), Stroke::new(3.0_f32, theme::FAINT));
            let origin = if self.bipolar { (self.default - self.min) / span } else { 0.0 };
            let from = angle(origin);
            let to = angle(t);
            arc(p, c, r, from.min(to), from.max(to), Stroke::new(3.0_f32, self.accent));

            p.circle_filled(c, r * 0.74, if active { theme::RAISED_HI } else { theme::RAISED });
            p.circle_stroke(c, r * 0.74, Stroke::new(1.0_f32, Color32::from_black_alpha(140)));
            let dir = vec2(to.cos(), to.sin());
            p.line_segment([c + dir * r * 0.22, c + dir * r * 0.66], Stroke::new(2.5_f32, theme::TEXT));

            let y = rect.top() + d + 1.0;
            p.text(pos2(rect.center().x, y), Align2::CENTER_TOP, self.label, FontId::proportional(12.5), theme::TEXT);
            p.text(
                pos2(rect.center().x, y + 15.0),
                Align2::CENTER_TOP,
                (self.format)(v),
                FontId::proportional(11.5),
                if active { self.accent } else { theme::DIM },
            );
        }
        resp
    }
}

struct KeyInput {
    steps: f32,
    reset: bool,
    to_min: bool,
    to_max: bool,
}

/// Clique (ou começar a arrastar) seleciona o widget; com ele selecionado, setas, PageUp/PageDown, Home/End,
/// Delete e a roda do mouse (só com o cursor em cima) viram passos. `None` quando não está selecionado.
fn selectable_input(ui: &mut Ui, resp: &Response) -> Option<KeyInput> {
    if resp.clicked() || resp.drag_started() {
        resp.request_focus();
    }
    if resp.clicked_elsewhere() && resp.has_focus() {
        resp.surrender_focus();
    }
    if !resp.has_focus() {
        return None;
    }
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            resp.id,
            egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, tab: false, escape: false },
        )
    });
    let hovered = resp.hovered();
    let wheel_id = resp.id.with("roda");
    let mut wheel_acc = ui.data(|d| d.get_temp::<f32>(wheel_id).unwrap_or(0.0));
    let keys = ui.input_mut(|i| {
        let unit = if i.modifiers.shift { 0.2 } else { 1.0 };
        let up = i.num_presses(Key::ArrowUp) + i.num_presses(Key::ArrowRight);
        let down = i.num_presses(Key::ArrowDown) + i.num_presses(Key::ArrowLeft);
        let mut steps = unit * (up as f32 - down as f32);
        steps += 10.0 * (i.num_presses(Key::PageUp) as f32 - i.num_presses(Key::PageDown) as f32);
        if hovered && i.raw_scroll_delta.y != 0.0 {
            wheel_acc += i.raw_scroll_delta.y;
            let notches = (wheel_acc / WHEEL_NOTCH).trunc();
            wheel_acc -= notches * WHEEL_NOTCH;
            steps += unit * notches.clamp(-5.0, 5.0);
            i.raw_scroll_delta = Vec2::ZERO;
            i.smooth_scroll_delta = Vec2::ZERO;
        }
        KeyInput {
            steps,
            reset: i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
            to_min: i.key_pressed(Key::Home),
            to_max: i.key_pressed(Key::End),
        }
    });
    ui.data_mut(|d| d.insert_temp(wheel_id, wheel_acc));
    Some(keys)
}

pub const FADER_W: f32 = 40.0;
const FADER_TRACK: f32 = 80.0;

/// Fader vertical de uma faixa do EQ (mais ou menos `range` dB). Arrastar ajusta (Shift = fino), duplo clique
/// ou botão direito zera; selecionado, responde ao mesmo teclado dos knobs (passo de 0,5 dB).
pub struct Fader<'a> {
    value: &'a mut f32,
    range: f32,
    label: String,
    accent: Color32,
}

impl<'a> Fader<'a> {
    pub fn new(value: &'a mut f32, range: f32, label: String, accent: Color32) -> Self {
        Self { value, range, label, accent }
    }
}

impl Widget for Fader<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, mut resp) = ui.allocate_exact_size(vec2(FADER_W, FADER_TRACK + 34.0), Sense::click_and_drag());
        let track = Rect::from_center_size(pos2(rect.center().x, rect.top() + FADER_TRACK / 2.0), vec2(4.0, FADER_TRACK - 14.0));
        let mut v = *self.value;

        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            v -= resp.drag_delta().y / track.height() * 2.0 * self.range / if fine { 5.0 } else { 1.0 };
        }
        if resp.double_clicked() || resp.secondary_clicked() {
            v = 0.0;
        }
        let keys = selectable_input(ui, &resp);
        let focused = keys.is_some();
        if let Some(k) = keys {
            v += k.steps * 0.5;
            if k.reset {
                v = 0.0;
            }
            if k.to_min {
                v = -self.range;
            }
            if k.to_max {
                v = self.range;
            }
        }
        v = v.clamp(-self.range, self.range);
        if v != *self.value {
            *self.value = v;
            resp.mark_changed();
        }

        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            let active = focused || resp.dragged() || resp.hovered();
            let y_of = |db: f32| track.center().y - db / self.range * track.height() / 2.0;
            let x = track.center().x;
            p.rect_filled(track, 2.0, theme::FAINT);
            p.line_segment([pos2(x - 8.0, y_of(0.0)), pos2(x + 8.0, y_of(0.0))], Stroke::new(1.0_f32, theme::DIM));
            let (a, b) = (y_of(0.0).min(y_of(v)), y_of(0.0).max(y_of(v)));
            p.rect_filled(Rect::from_min_max(pos2(x - 2.0, a), pos2(x + 2.0, b)), 2.0, self.accent);
            let cap = Rect::from_center_size(pos2(x, y_of(v)), vec2(24.0, 11.0));
            p.rect_filled(cap, 3.0, if active { theme::RAISED_HI } else { theme::RAISED });
            p.rect_stroke(cap, 3.0, Stroke::new(if focused { 1.5_f32 } else { 1.0 }, if focused { self.accent } else { theme::FAINT }));
            p.line_segment([pos2(cap.left() + 5.0, cap.center().y), pos2(cap.right() - 5.0, cap.center().y)], Stroke::new(1.5_f32, theme::TEXT));

            let y = rect.top() + FADER_TRACK + 1.0;
            p.text(pos2(x, y), Align2::CENTER_TOP, &self.label, FontId::proportional(11.5), theme::DIM);
            p.text(pos2(x, y + 15.0), Align2::CENTER_TOP, fmt_db(v).replace(" dB", ""), FontId::proportional(11.5), if active { self.accent } else { theme::TEXT });
        }
        resp
    }
}

fn arc(p: &Painter, c: Pos2, r: f32, a: f32, b: f32, stroke: Stroke) {
    let n = (((b - a) / (2.0 * PI)) * 72.0).ceil().max(2.0) as usize;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let t = a + (b - a) * i as f32 / n as f32;
            c + vec2(t.cos(), t.sin()) * r
        })
        .collect();
    p.add(Shape::line(pts, stroke));
}

pub fn led(ui: &mut Ui, on: bool, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    paint_led(ui.painter(), rect.center(), on, color);
    resp
}

fn paint_led(p: &Painter, c: Pos2, on: bool, color: Color32) {
    if on {
        p.circle_filled(c, 7.0, color.gamma_multiply(0.22));
        p.circle_filled(c, 4.0, color);
    } else {
        p.circle_filled(c, 4.0, theme::FAINT);
    }
}

/// Pedal de pisar: LED em cima, chave metálica embaixo. Clique alterna.
pub fn stomp(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(56.0, 46.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        paint_led(p, pos2(rect.center().x, rect.top() + 7.0), *on, theme::GREEN);
        let c = pos2(rect.center().x, rect.top() + 30.0);
        let hover = resp.hovered();
        p.circle_filled(c, 14.0, Color32::from_rgb(0x5d, 0x62, 0x6b));
        p.circle_filled(c, 11.0, if hover { Color32::from_rgb(0xd0, 0xd4, 0xda) } else { Color32::from_rgb(0xae, 0xb3, 0xbb) });
        p.circle_stroke(c, 7.0, Stroke::new(1.0_f32, Color32::from_black_alpha(70)));
    }
    resp.on_hover_text(if *on { "Ligado. Clique para desligar." } else { "Desligado. Clique para ligar." })
}

/// Barra de nível horizontal, -60 a 0 dBFS, com marca de pico.
pub fn level_bar(ui: &mut Ui, db: f32, hold_db: f32, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 9.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, theme::RAISED);
    let x_of = |db: f32| rect.left() + rect.width() * ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let level_x = x_of(db);
    for (lo, hi, color) in [(-60.0, -18.0, theme::GREEN), (-18.0, -6.0, theme::YELLOW), (-6.0, 0.0, theme::RED)] {
        let l = x_of(lo);
        let h = x_of(hi).min(level_x);
        if h > l {
            p.rect_filled(Rect::from_min_max(pos2(l, rect.top()), pos2(h, rect.bottom())), 2.0, color);
        }
    }
    if hold_db > -60.0 {
        let x = x_of(hold_db);
        p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0_f32, theme::TEXT));
    }
    resp
}

/// Corda-alvo do afinador: número (6 a 1), nota MIDI esperada e o desvio medido, se houver som.
#[derive(Clone, Copy)]
pub struct TunerTarget {
    pub string: u8,
    pub midi: i32,
    pub cents: Option<f32>,
    /// Corda escolhida pelo usuário (modo guiado), não adivinhada pelo automático.
    pub locked: bool,
    /// Com a corda travada: outra corda da afinação cuja nota bate com o som (número da corda).
    pub other_string: Option<u8>,
}

/// Afinador: nota da corda que está sendo afinada, escala de -50 a +50 cents e a dica do que fazer.
pub fn tuner_view(ui: &mut Ui, note: Option<Note>, target: Option<TunerTarget>, needle_cents: f32, notation: Notation) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
    let p = ui.painter_at(rect);
    let off = match target {
        Some(t) => t.cents,
        None => note.map(|n| n.cents),
    };
    let color = match off {
        None => theme::FAINT,
        Some(c) if c.abs() <= 5.0 => theme::GREEN,
        Some(c) if c.abs() <= 15.0 => theme::YELLOW,
        Some(_) => theme::RED,
    };

    let note_box = Rect::from_min_size(rect.min, vec2(150.0, rect.height()));
    let (big, small) = match (target, note) {
        (Some(t), _) => {
            let i = t.midi.rem_euclid(12) as usize;
            let octave = t.midi.div_euclid(12) - 1;
            (notation.name(i).to_string(), format!("{}ª corda · {}{}", t.string, notation.other().name(i), octave))
        }
        (None, Some(n)) => (notation.name(n.index).to_string(), format!("{}{}", notation.other().name(n.index), n.octave)),
        (None, None) => (String::new(), String::new()),
    };
    if big.is_empty() {
        p.text(note_box.center(), Align2::CENTER_CENTER, "toque uma corda", FontId::proportional(13.0), theme::DIM);
    } else {
        let big_color = if off.is_some() { color } else { theme::DIM };
        p.text(note_box.center() - vec2(0.0, 10.0), Align2::CENTER_CENTER, big, FontId::proportional(50.0), big_color);
        p.text(pos2(note_box.center().x, note_box.bottom() - 10.0), Align2::CENTER_CENTER, small, FontId::proportional(13.0), theme::DIM);
    }

    let scale = Rect::from_min_max(pos2(note_box.right() + 18.0, rect.top() + 18.0), pos2(rect.right() - 18.0, rect.top() + 52.0));
    let x_of = |c: f32| scale.left() + scale.width() * (c.clamp(-50.0, 50.0) + 50.0) / 100.0;
    p.rect_filled(Rect::from_min_max(pos2(x_of(-5.0), scale.top()), pos2(x_of(5.0), scale.bottom())), 3.0, theme::GREEN.gamma_multiply(0.18));
    for c in (-50..=50).step_by(5) {
        let x = x_of(c as f32);
        let h = if c == 0 { 1.0 } else if c % 25 == 0 { 0.7 } else if c % 10 == 0 { 0.5 } else { 0.25 };
        let top = scale.center().y - scale.height() * h / 2.0;
        let bottom = scale.center().y + scale.height() * h / 2.0;
        p.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(if c == 0 { 2.0_f32 } else { 1.0 }, theme::FAINT));
    }
    for c in [-50, -25, 0, 25, 50] {
        let label = if c > 0 { format!("+{c}") } else { c.to_string() };
        p.text(pos2(x_of(c as f32), scale.bottom() + 4.0), Align2::CENTER_TOP, label, FontId::proportional(10.5), theme::DIM);
    }

    let text_pos = pos2(scale.left(), rect.bottom() - 6.0);
    match (off, target) {
        (Some(c), _) => {
            let x = x_of(needle_cents);
            p.line_segment([pos2(x, scale.top() - 6.0), pos2(x, scale.bottom() + 2.0)], Stroke::new(4.0_f32, color));
            p.circle_filled(pos2(x, scale.top() - 8.0), 5.0, color);
            let advice = if c.abs() <= 5.0 {
                "Afinado!".to_string()
            } else if c > 200.0 {
                match target {
                    Some(t) => format!(
                        "Muito acima da {0}ª corda. Se é ela que está soando, afrouxe já (pode arrebentar); se tocou outra, toque só a {0}ª",
                        t.string
                    ),
                    None => "Muito acima: afrouxe já, corda esticada demais pode arrebentar".to_string(),
                }
            } else if c < -50.0 {
                "Bem abaixo: aperte a tarraxa".to_string()
            } else if c < 0.0 {
                "Um pouco baixo: aperte devagar".to_string()
            } else if c > 50.0 {
                "Bem acima: afrouxe a tarraxa".to_string()
            } else {
                "Um pouco alto: afrouxe devagar".to_string()
            };
            let wrong_string_hint = match target {
                Some(t) if !t.locked && c.abs() > 100.0 => " · não é essa corda? clique na corda certa acima",
                _ => "",
            };
            let playing = match (target, note) {
                (Some(_), Some(n)) => format!(" · tocando {}{} ({:.1} Hz)", notation.name(n.index), n.octave, n.freq),
                (None, Some(n)) => format!(" · {:.1} Hz", n.freq),
                _ => String::new(),
            };
            // corda escolhida, mas o som é o de outra corda: em vez do alarme, pergunta qual foi
            let text = match target {
                Some(t) if t.locked && c.abs() > 100.0 && t.other_string.is_some() => {
                    let o = t.other_string.unwrap_or_default();
                    let (state, action) = if c > 0.0 { ("esticada demais", "afrouxe") } else { ("frouxa demais", "aperte") };
                    format!(
                        "Esse é o som da {o}ª corda. Tocou a {o}ª? Clique em {o}ª acima. Se foi a {}ª, ela está {state}: {action}{playing}",
                        t.string
                    )
                }
                _ => format!("{advice}   {c:+.0} cents{playing}{wrong_string_hint}"),
            };
            let color = match target {
                Some(t) if t.locked && c.abs() > 100.0 && t.other_string.is_some() => theme::YELLOW,
                _ => color,
            };
            p.text(text_pos, Align2::LEFT_BOTTOM, text, FontId::proportional(13.0), color);
        }
        (None, Some(t)) => {
            p.text(text_pos, Align2::LEFT_BOTTOM, format!("Toque a {}ª corda", t.string), FontId::proportional(13.0), theme::DIM);
        }
        (None, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, RawInput};

    fn frame(ctx: &egui::Context, events: Vec<Event>, value: &mut f32) {
        // na janela real o egui-winit manda o Shift no estado do quadro e no evento; aqui copiamos do evento
        let modifiers = events
            .iter()
            .find_map(|e| if let Event::Key { modifiers, .. } = e { Some(*modifiers) } else { None })
            .unwrap_or_default();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
            events,
            modifiers,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(Knob::new(value, "Teste"));
            });
        });
    }

    fn key(k: Key, shift: bool) -> Event {
        let modifiers = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn wheel(notches: f32) -> Event {
        Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, notches), modifiers: Modifiers::NONE }
    }

    #[test]
    fn knob_wheel_needs_selection_and_keys_step() {
        let ctx = egui::Context::default();
        let mut v = 0.5;
        frame(&ctx, vec![], &mut v);
        let at = pos2(43.0, 33.0);
        frame(&ctx, vec![Event::PointerMoved(at), wheel(1.0)], &mut v);
        assert_eq!(v, 0.5, "roda com o cursor só passando não pode mexer");

        let click = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&ctx, vec![click(true)], &mut v);
        frame(&ctx, vec![click(false)], &mut v);
        frame(&ctx, vec![], &mut v);

        frame(&ctx, vec![key(Key::ArrowUp, false)], &mut v);
        assert!((v - 0.51).abs() < 1e-5, "seta para cima: {v}");
        frame(&ctx, vec![key(Key::ArrowDown, true)], &mut v);
        assert!((v - 0.508).abs() < 1e-5, "Shift + seta para baixo: {v}");
        frame(&ctx, vec![wheel(1.0)], &mut v);
        assert!((v - 0.518).abs() < 1e-5, "um clique da roda: {v}");
        frame(&ctx, vec![key(Key::PageUp, false)], &mut v);
        assert!((v - 0.618).abs() < 1e-5, "PageUp: {v}");
        frame(&ctx, vec![key(Key::Delete, false)], &mut v);
        assert_eq!(v, 0.5, "Delete volta ao padrão");
    }

    #[test]
    fn knob_values_read_like_an_amp() {
        assert_eq!(fmt_ten(0.5), "5.0");
        assert_eq!(fmt_ten(0.0), "0.0");
        assert_eq!(fmt_ten(0.505), "5.05");
        assert_eq!(fmt_db(-52.0), "-52 dB");
        assert_eq!(fmt_db(0.5), "+0.5 dB");
        assert_eq!(fmt_db(0.0), "+0 dB");
    }
}
