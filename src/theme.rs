//! App appearance is independent of experiments and their rendered output.
use eframe::egui::{self, Color32, Theme, ThemePreference};

const STORAGE_KEY: &str = "nodiform.appearance";

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub surface: Color32,
    pub card: Color32,
    pub window: Color32,
    pub ink: Color32,
    pub secondary: Color32,
    pub line: Color32,
    pub accent: Color32,
    pub action: Color32,
    pub error: Color32,
    pub inset: Color32,
    pub selected: Color32,
    pub editor: Color32,
    pub gutter: Color32,
    pub keyword: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub switch_off: Color32,
}

impl Palette {
    pub fn for_ui(ui: &egui::Ui) -> Self {
        Self::new(ui.visuals().dark_mode)
    }

    pub fn for_ctx(ctx: &egui::Context) -> Self {
        Self::new(ctx.style().visuals.dark_mode)
    }

    pub fn new(dark: bool) -> Self {
        let rgb = Color32::from_rgb;
        if dark {
            Self {
                surface: rgb(19, 24, 33),
                card: rgb(30, 37, 49),
                window: rgb(32, 40, 53),
                ink: rgb(233, 239, 249),
                secondary: rgb(165, 180, 202),
                line: rgb(61, 73, 92),
                accent: rgb(115, 181, 255),
                action: rgb(0, 112, 245),
                error: rgb(255, 135, 145),
                inset: rgb(22, 29, 40),
                selected: rgb(55, 70, 94),
                editor: rgb(23, 30, 41),
                gutter: rgb(124, 143, 169),
                keyword: rgb(214, 158, 255),
                string: rgb(125, 224, 167),
                number: rgb(255, 190, 123),
                comment: rgb(148, 170, 193),
                switch_off: rgb(76, 90, 111),
            }
        } else {
            Self {
                surface: rgb(240, 243, 249),
                card: rgb(255, 255, 255),
                window: rgb(250, 252, 255),
                ink: rgb(29, 37, 54),
                secondary: rgb(106, 116, 135),
                line: rgb(218, 225, 236),
                accent: rgb(0, 103, 183),
                action: rgb(0, 112, 245),
                error: rgb(179, 48, 59),
                inset: rgb(233, 238, 247),
                selected: rgb(255, 255, 255),
                editor: rgb(255, 255, 254),
                gutter: rgb(126, 135, 151),
                keyword: rgb(135, 54, 165),
                string: rgb(43, 116, 66),
                number: rgb(174, 91, 28),
                comment: rgb(103, 116, 132),
                switch_off: rgb(214, 219, 228),
            }
        }
    }
}

pub(crate) fn restore(storage: Option<&dyn eframe::Storage>) -> ThemePreference {
    match storage
        .and_then(|storage| storage.get_string(STORAGE_KEY))
        .as_deref()
    {
        Some("light") => ThemePreference::Light,
        Some("dark") => ThemePreference::Dark,
        _ => ThemePreference::System,
    }
}

pub(crate) fn save(storage: &mut dyn eframe::Storage, preference: ThemePreference) {
    storage.set_string(
        STORAGE_KEY,
        match preference {
            ThemePreference::System => "system",
            ThemePreference::Light => "light",
            ThemePreference::Dark => "dark",
        }
        .into(),
    );
}

pub(crate) fn configure(ctx: &egui::Context, preference: ThemePreference) {
    for theme in [Theme::Light, Theme::Dark] {
        let dark = theme == Theme::Dark;
        let p = Palette::new(dark);
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = p.surface;
        visuals.window_fill = p.window;
        visuals.extreme_bg_color = p.editor;
        visuals.faint_bg_color = p.inset;
        visuals.override_text_color = Some(p.ink);
        visuals.selection.bg_fill = if dark {
            Color32::from_rgb(42, 75, 119)
        } else {
            Color32::from_rgb(206, 226, 255)
        };
        visuals.selection.stroke.color = p.ink;
        visuals.hyperlink_color = p.accent;
        visuals.warn_fg_color = if dark {
            Color32::from_rgb(255, 194, 102)
        } else {
            Color32::from_rgb(133, 92, 0)
        };
        visuals.error_fg_color = p.error;
        visuals.window_corner_radius = 22.into();
        visuals.window_stroke = egui::Stroke::new(1.0_f32, p.line);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, p.line);
        visuals.widgets.noninteractive.fg_stroke.color = p.ink;
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = 12.into();
            widget.fg_stroke.color = p.ink;
            widget.bg_stroke = egui::Stroke::new(1.0_f32, p.line);
        }
        let hovered = if dark {
            Color32::from_rgb(49, 65, 87)
        } else {
            Color32::from_rgb(226, 236, 252)
        };
        let active = if dark {
            Color32::from_rgb(48, 83, 126)
        } else {
            Color32::from_rgb(210, 228, 254)
        };
        visuals.widgets.inactive.bg_fill = p.card;
        visuals.widgets.inactive.weak_bg_fill = p.card;
        visuals.widgets.hovered.bg_fill = hovered;
        visuals.widgets.hovered.weak_bg_fill = hovered;
        visuals.widgets.active.bg_fill = active;
        visuals.widgets.active.weak_bg_fill = active;
        visuals.widgets.open.bg_fill = hovered;
        visuals.widgets.open.weak_bg_fill = hovered;
        ctx.style_mut_of(theme, |style| {
            style.visuals = visuals;
            style.spacing.item_spacing = egui::vec2(10.0, 9.0);
            style.spacing.button_padding = egui::vec2(16.0, 9.0);
            style.spacing.interact_size.y = 32.0;
            for (text, size) in [
                (egui::TextStyle::Body, 14.0),
                (egui::TextStyle::Button, 14.0),
                (egui::TextStyle::Small, 12.0),
                (egui::TextStyle::Heading, 22.0),
            ] {
                style
                    .text_styles
                    .insert(text, egui::FontId::proportional(size));
            }
        });
    }
    ctx.options_mut(|options| options.fallback_theme = Theme::Light);
    ctx.set_theme(preference);
}
