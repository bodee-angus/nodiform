use super::*;

const SURFACE: egui::Color32 = egui::Color32::from_rgb(240, 243, 249);
const INK: egui::Color32 = egui::Color32::from_rgb(29, 37, 54);
const LINE: egui::Color32 = egui::Color32::from_rgb(218, 225, 236);
const CANVAS: egui::Color32 = egui::Color32::from_rgb(10, 18, 24);

pub(super) fn configure(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = SURFACE;
    visuals.window_fill = egui::Color32::from_rgba_unmultiplied(250, 252, 255, 250);
    visuals.extreme_bg_color = egui::Color32::from_rgb(248, 250, 254);
    visuals.faint_bg_color = egui::Color32::from_rgb(231, 237, 246);
    visuals.override_text_color = Some(INK);
    visuals.selection.bg_fill = egui::Color32::from_rgb(206, 226, 255);
    visuals.selection.stroke.color = INK;
    visuals.hyperlink_color = ACCENT;
    visuals.window_corner_radius = 22.into();
    visuals.window_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    visuals.widgets.noninteractive.fg_stroke.color = INK;
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = 12.into();
        widget.fg_stroke.color = INK;
        widget.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    }
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(251, 252, 255);
    visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(251, 252, 255);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(226, 236, 252);
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(210, 228, 254);
    visuals.widgets.active.weak_bg_fill = visuals.widgets.active.bg_fill;
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 9.0);
        style.spacing.button_padding = egui::vec2(16.0, 9.0);
        style.spacing.interact_size.y = 32.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));
    });
}

fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 225))
        .stroke(egui::Stroke::new(1.0_f32, egui::Color32::WHITE))
        .corner_radius(20)
        .inner_margin(16)
}

fn button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text).corner_radius(100)
}

fn logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 12, egui::Color32::WHITE);
    let points = [
        rect.min + egui::vec2(11.0, 12.0),
        rect.min + egui::vec2(28.0, 18.0),
        rect.min + egui::vec2(15.0, 29.0),
    ];
    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
        painter.line_segment([points[a], points[b]], egui::Stroke::new(1.7_f32, ACCENT));
    }
    for point in points {
        painter.circle_filled(point, 3.5, ACCENT);
    }
}

impl NodiformApp {
    pub(super) fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(22, 16)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    logo(ui);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Nodiform").size(24.0).strong());
                        ui.label(
                            egui::RichText::new("A place for emergent ideas")
                                .size(11.0)
                                .color(MUTED),
                        );
                    });
                    ui.add_space(14.0);
                    let idle = !self.busy();
                    ui.add_enabled_ui(idle, |ui| {
                        ui.menu_button("Experiment", |ui| {
                            if ui.button("New experiment").clicked() {
                                self.confirm_example = Some("starter");
                                ui.close_menu();
                            }
                            if ui.button("Open…").clicked() {
                                self.open();
                                ui.close_menu();
                            }
                            if ui.button("Save…").clicked() {
                                self.save();
                                ui.close_menu();
                            }
                            ui.separator();
                            ui.menu_button("Examples", |ui| {
                                for (id, label) in [
                                    ("complete", "Connect to every earlier node"),
                                    ("ring", "Growing ring"),
                                    ("abc", "Letter permutations"),
                                    ("modular", "Modular residues"),
                                ] {
                                    if ui.button(label).clicked() {
                                        self.confirm_example = Some(id);
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(button("Settings")).clicked() {
                            self.show_settings = !self.show_settings;
                        }
                        if ui
                            .add_enabled(idle, button("Record"))
                            .on_hover_text("Run the experiment and save every frame to video")
                            .clicked()
                        {
                            self.request(Intent::Record);
                        }
                        if ui
                            .add_enabled(
                                idle,
                                egui::Button::new(
                                    egui::RichText::new("Preview").color(egui::Color32::WHITE),
                                )
                                .corner_radius(100)
                                .fill(ACCENT)
                                .stroke(egui::Stroke::NONE),
                            )
                            .on_hover_text("Run without recording · Ctrl+Enter")
                            .clicked()
                        {
                            self.request(Intent::Preview);
                        }
                        if ui
                            .add_enabled(idle, button("Validate"))
                            .on_hover_text("Check the rules and generated graph")
                            .clicked()
                        {
                            self.request(Intent::Validate);
                        }
                    });
                });
            });
    }

    pub(super) fn side_panel(&mut self, ctx: &egui::Context) {
        let enabled = !self.busy();
        egui::SidePanel::left("rules")
            .resizable(true)
            .default_width(570.0)
            .width_range(420.0..=900.0)
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(egui::Margin {
                left: 22,
                right: 10,
                top: 0,
                bottom: 12,
            }))
            .show(ctx, |ui| {
                card().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Experiment").size(20.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("JavaScript").size(12.0).color(MUTED));
                        });
                    });
                    ui.label(
                        egui::RichText::new("Describe what happens. Explore what emerges.")
                            .color(MUTED),
                    );
                    ui.add_space(8.0);
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(233, 238, 247))
                        .corner_radius(12)
                        .inner_margin(3)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (tab, label) in
                                    [(EditorTab::Rules, "Rules"), (EditorTab::Inputs, "Inputs")]
                                {
                                    let selected = self.editor_tab == tab;
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(label).color(if selected {
                                                    INK
                                                } else {
                                                    MUTED
                                                }),
                                            )
                                            .fill(if selected {
                                                egui::Color32::WHITE
                                            } else {
                                                egui::Color32::TRANSPARENT
                                            })
                                            .stroke(egui::Stroke::NONE)
                                            .corner_radius(10)
                                            .min_size(egui::vec2(110.0, 30.0)),
                                        )
                                        .clicked()
                                    {
                                        self.editor_tab = tab;
                                    }
                                }
                            });
                        });
                });
                ui.add_space(10.0);
                match self.editor_tab {
                    EditorTab::Rules => {
                        editor::show(ui, &mut self.project.source, enabled);
                    }
                    EditorTab::Inputs => {
                        crate::inputs::show(
                            ui,
                            &self.project.source,
                            &mut self.parameters_text,
                            enabled,
                        );
                    }
                }
            });
    }

    pub(super) fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(egui::Margin {
                left: 10,
                right: 22,
                top: 0,
                bottom: 12,
            }))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Simulation").size(20.0).strong());
                    let state = if self.paused {
                        "Paused"
                    } else if self.busy() {
                        "Running"
                    } else {
                        "Ready"
                    };
                    ui.label(egui::RichText::new(state).size(12.0).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(button("Inspect")).clicked() {
                            self.show_inspector = !self.show_inspector;
                        }
                        ui.label(egui::RichText::new("2D · Auto fit").size(12.0).color(MUTED));
                    });
                });
                ui.add_space(6.0);
                let available = egui::vec2(
                    ui.available_width(),
                    (ui.available_height() - 123.0).max(150.0),
                );
                let (rect, _) = ui.allocate_exact_size(available, egui::Sense::hover());
                ui.painter().rect_filled(rect, 24, CANVAS);
                let inner = rect.shrink(16.0);
                let aspect = self.gpu.dimensions().0 as f32 / self.gpu.dimensions().1 as f32;
                let size = if inner.width() / inner.height() > aspect {
                    egui::vec2(inner.height() * aspect, inner.height())
                } else {
                    egui::vec2(inner.width(), inner.width() / aspect)
                };
                let image_rect = egui::Rect::from_center_size(inner.center(), size);
                ui.painter().image(
                    self.texture,
                    image_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                if self.graph.nodes.is_empty() {
                    ui.painter().text(
                        rect.center() - egui::vec2(0.0, 16.0),
                        egui::Align2::CENTER_CENTER,
                        "A little curiosity goes a long way.",
                        egui::FontId::proportional(20.0),
                        egui::Color32::from_rgb(222, 233, 246),
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0.0, 18.0),
                        egui::Align2::CENTER_CENTER,
                        "Write your rules, then choose Preview.",
                        egui::FontId::proportional(14.0),
                        egui::Color32::from_rgb(143, 160, 181),
                    );
                }
                ui.add_space(10.0);
                card().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (number, label) in [
                            (self.graph.nodes.len() as u64, "Nodes"),
                            (self.graph.edges.len() as u64, "Edges"),
                            (self.tick, "Ticks"),
                        ] {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(number.to_string()).size(22.0).strong(),
                                );
                                ui.label(egui::RichText::new(label).size(11.0).color(MUTED));
                            });
                            ui.add_space(22.0);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(self.busy(), button("Stop")).clicked() {
                                self.stop();
                            }
                            if ui
                                .add_enabled(
                                    self.paused
                                        && self.pending_frame.is_none()
                                        && !self.finishing
                                        && self.timeline.as_ref().is_some_and(|t| !t.finished()),
                                    button("Step"),
                                )
                                .on_hover_text("Advance one output-frame interval")
                                .clicked()
                            {
                                if let Err(error) = self.advance_frame() {
                                    self.error = Some(error);
                                    self.stop();
                                }
                            }
                            if ui
                                .add_enabled(
                                    self.timeline.as_ref().is_some_and(|t| !t.finished())
                                        && !self.finishing,
                                    button(if self.paused { "Resume" } else { "Pause" }),
                                )
                                .clicked()
                            {
                                self.paused = !self.paused;
                            }
                        });
                    });
                });
            });
    }

    pub(super) fn status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(24, 9)),
            )
            .show(ctx, |ui| {
                if let Some(error) = &self.error {
                    egui::ScrollArea::vertical()
                        .id_salt("error_details")
                        .max_height(90.0)
                        .show(ui, |ui| {
                            ui.colored_label(egui::Color32::from_rgb(179, 48, 59), error);
                        });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(&self.status).size(12.0).color(MUTED));
                    if let Some(recorder) = &self.recorder {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} frames {}",
                                recorder.frames(),
                                if self.finishing {
                                    "· finishing"
                                } else {
                                    "· recorded"
                                }
                            ))
                            .size(12.0)
                            .color(ACCENT),
                        )
                        .on_hover_text(recorder.directory().display().to_string());
                    }
                });
            });
    }

    pub(super) fn settings(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        egui::Window::new("Settings").open(&mut open).resizable(false).default_width(390.0).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 90.0)).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height() - 160.0).max(240.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("Appearance").strong());
                let recording = self.pending_run.as_ref().is_some_and(|(intent, _)| *intent == Intent::Record) || self.recorder.is_some() || self.recorder_start.is_some() || self.finishing;
                if ui.add_enabled(!recording, egui::Checkbox::new(&mut self.project.size_by_connections, "Size nodes by connections")).changed() {
                    self.gpu.set_degree_sizing(self.project.size_by_connections);
                    if let Some((_, snapshot)) = &mut self.pending_run {
                        snapshot.size_by_connections = self.project.size_by_connections;
                    }
                    let (width, height) = self.gpu.dimensions();
                    self.gpu.render(width, height);
                }
                ui.label(egui::RichText::new("Connection count changes appearance only. Node sizes still follow the zoom level.").size(12.0).color(MUTED));
                ui.add_space(10.0);
                ui.separator();
                ui.add_enabled_ui(!self.busy(), |ui| {
                    ui.label(egui::RichText::new("Simulation").strong());
                    ui.horizontal(|ui| { ui.label("Random seed"); ui.add(egui::DragValue::new(&mut self.project.seed)); });
                    ui.horizontal(|ui| { ui.label("Final settling ticks"); ui.add(egui::DragValue::new(&mut self.project.tail_ticks).range(0..=10_000_000)); });
                    ui.label(egui::RichText::new("Repulsion and edge attraction. No central gravity.").size(12.0).color(MUTED));
                    ui.add_space(10.0);
                    ui.separator();
                    ui.label(egui::RichText::new("Recording").strong());
                    egui::ComboBox::from_id_salt("resolution").selected_text(format!("{} × {}", self.project.recording.width, self.project.recording.height)).show_ui(ui, |ui| {
                        for (width, height) in [(1280, 720), (1920, 1080), (3840, 2160)] {
                            if ui.selectable_label(self.project.recording.width == width, format!("{width} × {height}")).clicked() { self.project.recording.width = width; self.project.recording.height = height; }
                        }
                    });
                    ui.horizontal(|ui| { ui.label("Frames per second"); ui.add(egui::DragValue::new(&mut self.project.recording.fps).range(1..=120)); });
                    ui.horizontal(|ui| { ui.label("Ticks per frame"); ui.add(egui::DragValue::new(&mut self.project.ticks_per_frame).range(1..=240)); });
                    egui::ComboBox::from_id_salt("codec").selected_text(if self.project.recording.codec == "h264_nvenc" { "H.264 · NVIDIA" } else { "H.264 · High quality" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.project.recording.codec, "libx264".into(), "H.264 · High quality");
                        ui.selectable_value(&mut self.project.recording.codec, "h264_nvenc".into(), "H.264 · NVIDIA");
                    });
                    if ui.button("Choose video folder…").clicked() { if let Some(path) = rfd::FileDialog::new().pick_folder() { self.output_dir = path; } }
                    ui.label(egui::RichText::new(self.output_dir.display().to_string()).size(12.0).color(MUTED));
                    ui.label(egui::RichText::new("Recording requires FFmpeg on this computer.").size(12.0).color(MUTED));
                });
                ui.add_space(10.0);
                ui.separator();
                ui.label(egui::RichText::new(&self.adapter).size(11.0).color(MUTED));
                ui.label(egui::RichText::new(format!("Nodiform {} · Experimental alpha", env!("CARGO_PKG_VERSION"))).size(11.0).color(MUTED));
            });
        });
        self.show_settings = open;
    }

    pub(super) fn inspector_window(&mut self, ctx: &egui::Context) {
        if !self.show_inspector {
            return;
        }
        let mut open = self.show_inspector;
        egui::Window::new("Inspector")
            .open(&mut open)
            .default_width(350.0)
            .show(ctx, |ui| {
                ui.label(format!("{} connected components", self.components));
                if self.components > 1 {
                    ui.label("Disconnected components can drift apart without gravity.");
                }
                if self.graph.nodes.is_empty() {
                    ui.label("Run an experiment to inspect its nodes.");
                    return;
                }
                self.inspector = self.inspector.min(self.graph.nodes.len() - 1);
                ui.horizontal(|ui| {
                    ui.label("Node index");
                    if ui
                        .add(
                            egui::DragValue::new(&mut self.inspector)
                                .range(0..=self.graph.nodes.len() - 1),
                        )
                        .changed()
                    {
                        self.inspected_position = None;
                    }
                });
                let node = &self.graph.nodes[self.inspector];
                ui.label(egui::RichText::new(&node.label).strong());
                ui.label(format!(
                    "Base radius {} · {} edges",
                    node.radius,
                    self.graph
                        .edges
                        .iter()
                        .filter(
                            |edge| edge.source == self.inspector || edge.target == self.inspector
                        )
                        .count()
                ));
                if ui
                    .add_enabled(self.paused || !self.busy(), button("Read position"))
                    .clicked()
                {
                    match self.gpu.read_positions(self.graph.nodes.len()) {
                        Ok(positions) => {
                            self.inspected_position = positions.get(self.inspector).copied()
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                if let Some([x, y]) = self.inspected_position {
                    ui.label(format!("x {x:.3}    y {y:.3}"));
                }
            });
        self.show_inspector = open;
    }

    pub(super) fn load_example(&mut self, id: &str) {
        let source = match id {
            "complete" => include_str!("../../examples/complete-growth.js"),
            "abc" => include_str!("../../examples/abc-permutations.js"),
            "ring" => include_str!("../../examples/ring.js"),
            "modular" => include_str!("../../examples/modular-residues.js"),
            _ => include_str!("../../examples/starter.js"),
        };
        let appearance = self.project.size_by_connections;
        self.project = Project {
            source: source.into(),
            size_by_connections: appearance,
            ..Project::default()
        };
        self.parameters_text = "{}".into();
        self.project_path = None;
        self.editor_tab = EditorTab::Rules;
        self.error = None;
        self.confirm_example = None;
        self.status = "Script loaded. All generation behaviour is defined in its rules.".into();
    }
}
