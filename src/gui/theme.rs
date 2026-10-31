use egui::{Color32, Context, FontId, Rounding, Stroke, TextStyle, Theme, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x14, 0x16, 0x1b);
pub const PANEL: Color32 = Color32::from_rgb(0x1b, 0x1e, 0x24);
pub const CARD: Color32 = Color32::from_rgb(0x25, 0x29, 0x31);
pub const CARD_OFF: Color32 = Color32::from_rgb(0x1f, 0x22, 0x28);
pub const RAISED: Color32 = Color32::from_rgb(0x34, 0x39, 0x43);
pub const RAISED_HI: Color32 = Color32::from_rgb(0x42, 0x48, 0x54);
pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xea, 0xe6);
pub const DIM: Color32 = Color32::from_rgb(0x93, 0x99, 0xa3);
pub const FAINT: Color32 = Color32::from_rgb(0x48, 0x4e, 0x58);
pub const AMBER: Color32 = Color32::from_rgb(0xff, 0xb3, 0x47);
pub const GREEN: Color32 = Color32::from_rgb(0x5c, 0xd6, 0x7a);
pub const YELLOW: Color32 = Color32::from_rgb(0xf2, 0xc1, 0x4e);
pub const RED: Color32 = Color32::from_rgb(0xff, 0x5a, 0x5a);

pub const GATE: Color32 = Color32::from_rgb(0xb4, 0xbc, 0xc8);
pub const COMP: Color32 = Color32::from_rgb(0x5a, 0xa9, 0xff);
pub const OVERDRIVE: Color32 = Color32::from_rgb(0x7b, 0xd8, 0x8f);
pub const DISTORTION: Color32 = Color32::from_rgb(0xff, 0x8a, 0x3d);
pub const FUZZ: Color32 = Color32::from_rgb(0xe0, 0x6a, 0xd8);
pub const CHORUS: Color32 = Color32::from_rgb(0x4f, 0xd1, 0xd9);
pub const CAB: Color32 = Color32::from_rgb(0xd6, 0xa8, 0x72);
pub const EQ: Color32 = Color32::from_rgb(0xc9, 0xe2, 0x65);
pub const DELAY: Color32 = Color32::from_rgb(0xa9, 0x8b, 0xff);
pub const REVERB: Color32 = Color32::from_rgb(0x44, 0xc4, 0xa1);
pub const WAH: Color32 = Color32::from_rgb(0xf5, 0xd0, 0x3b);
pub const OCTAVE: Color32 = Color32::from_rgb(0x7f, 0x8c, 0xff);
pub const BOOST: Color32 = Color32::from_rgb(0xff, 0x5f, 0x7e);
pub const PHASER: Color32 = Color32::from_rgb(0xff, 0x8f, 0xc8);
pub const FLANGER: Color32 = Color32::from_rgb(0xd9, 0x9b, 0xff);
pub const TREMOLO: Color32 = Color32::from_rgb(0x9b, 0xe3, 0x7a);

/// Tema escuro fixo. As cores ficam todas aqui para a fase "personalizável".
pub fn apply(ctx: &Context) {
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, |style| {
        let mut v = Visuals::dark();
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.faint_bg_color = CARD;
        v.override_text_color = Some(TEXT);
        v.window_rounding = Rounding::same(10.0);
        v.window_stroke = Stroke::new(1.0_f32, FAINT);
        v.selection.bg_fill = AMBER.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0_f32, AMBER);
        v.hyperlink_color = AMBER;
        for (w, fill) in [
            (&mut v.widgets.noninteractive, CARD),
            (&mut v.widgets.inactive, RAISED),
            (&mut v.widgets.hovered, RAISED_HI),
            (&mut v.widgets.active, Color32::from_rgb(0x4e, 0x55, 0x63)),
            (&mut v.widgets.open, RAISED_HI),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.rounding = Rounding::same(6.0);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, FAINT);
        style.visuals = v;

        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Heading, FontId::proportional(19.0));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(11.5));
    });
}
