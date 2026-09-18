use crate::{
    diagnostics::SmokeProbe,
    editor,
    gpu::GpuGraph,
    model::{Graph, Plan},
    recording::{Recorder, RecorderStartJob, RecordingConfig},
    rules::RuleJob,
    timeline::Timeline,
};
use eframe::{egui, wgpu};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const MINT: egui::Color32 = egui::Color32::from_rgb(121, 223, 199);
const MUTED: egui::Color32 = egui::Color32::from_rgb(142, 164, 171);

#[derive(Clone, Serialize, Deserialize)]
struct Project {
    schema: u32,
    source: String,
    parameters: Value,
    seed: u32,
    ticks_per_frame: u32,
    tail_ticks: u32,
    recording: RecordingConfig,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            schema: 1,
            source: include_str!("../examples/abc-permutations.js").into(),
            parameters: json!({"alphabet":"ABC", "maxLength":3, "repetitions":false, "order":"lexicographic", "ticksPerNode":24, "finalTicks":0}),
            seed: 42,
            ticks_per_frame: 4,
            tail_ticks: 240,
            recording: RecordingConfig {
                width: 1280,
                height: 720,
                fps: 60,
                codec: "libx264".into(),
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Intent {
    Validate,
    Preview,
    Record,
}

pub struct NodiformApp {
    project: Project,
    parameters_text: String,
    project_path: Option<PathBuf>,
    output_dir: PathBuf,
    graph: Graph,
    gpu: GpuGraph,
    render_state: eframe::egui_wgpu::RenderState,
    texture: egui::TextureId,
    timeline: Option<Timeline>,
    job: Option<RuleJob>,
    pending_run: Option<(Intent, Project)>,
    recorder: Option<Recorder>,
    recorder_start: Option<RecorderStartJob>,
    prepared_run: Option<(Project, Plan)>,
    pending_frame: Option<Vec<u8>>,
    finishing: bool,
    paused: bool,
    tick: u64,
    last_event: usize,
    components: usize,
    status: String,
    error: Option<String>,
    last_sample: Instant,
    adapter: String,
    inspector: usize,
    inspected_position: Option<[f32; 2]>,
    confirm_example: Option<&'static str>,
    smoke_probe: Option<SmokeProbe>,
}

impl NodiformApp {
    pub fn new(cc: &eframe::CreationContext<'_>, smoke_probe: Option<SmokeProbe>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(20, 35, 43);
        visuals.window_fill = visuals.panel_fill;
        visuals.extreme_bg_color = egui::Color32::from_rgb(13, 23, 29);
        visuals.override_text_color = Some(egui::Color32::from_rgb(224, 235, 232));
        visuals.selection.bg_fill = egui::Color32::from_rgb(42, 85, 82);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(43, 103, 91);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(10.0, 9.0);
            s.spacing.button_padding = egui::vec2(13.0, 8.0);
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        });
        let render_state = cc
            .wgpu_render_state
            .clone()
            .expect("Nodiform requires a compute-capable GPU");
        let info = render_state.adapter.get_info();
        let adapter = format!("{} · {:?}", info.name, info.backend);
        let mut gpu = GpuGraph::new(
            Arc::new(render_state.device.clone()),
            Arc::new(render_state.queue.clone()),
        );
        let graph = Graph::new(42);
        gpu.sync_graph(&graph, true);
        gpu.render(1280, 720);
        let texture = render_state.renderer.write().register_native_texture(
            &render_state.device,
            gpu.view(),
            wgpu::FilterMode::Linear,
        );
        let project = Project::default();
        let mut app = Self {
            parameters_text: serde_json::to_string_pretty(&project.parameters).unwrap(),
            project,
            project_path: None,
            output_dir: std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join("Videos/Nodiform"))
                .unwrap_or_else(|| PathBuf::from("recordings")),
            graph,
            gpu,
            render_state,
            texture,
            timeline: None,
            job: None,
            pending_run: None,
            recorder: None,
            recorder_start: None,
            prepared_run: None,
            pending_frame: None,
            finishing: false,
            paused: false,
            tick: 0,
            last_event: 0,
            components: 0,
            status: "Start with the ABC experiment. Validate, then preview its growth.".into(),
            error: None,
            last_sample: Instant::now(),
            adapter,
            inspector: 0,
            inspected_position: None,
            confirm_example: None,
            smoke_probe,
        };
        if app.smoke_probe.is_some() {
            // Exercise the actual child process, timeline and rendering path.
            // Preview never creates a project, recording or output directory.
            app.request(Intent::Preview);
        }
        app
    }

    fn busy(&self) -> bool {
        self.job.is_some()
            || self.recorder_start.is_some()
            || self.recorder.is_some()
            || self.timeline.as_ref().is_some_and(|t| !t.finished())
    }

    fn check_smoke_test(&mut self, ctx: &egui::Context) {
        let Some(probe) = &mut self.smoke_probe else {
            return;
        };
        if probe.completed {
            return;
        }
        let ready = probe.ready(self.tick, self.graph.nodes.len(), self.graph.edges.len());
        if !ready && self.error.is_none() {
            ctx.request_repaint_after(Duration::from_millis(8));
            return;
        }
        let frames = probe.frames;
        let result = if let Some(error) = &self.error {
            Err(error.clone())
        } else {
            self.validate_smoke_frame().map(|()| {
                format!(
                    "{} GUI updates, {} ticks, {} nodes, {} edges; {}",
                    frames,
                    self.tick,
                    self.graph.nodes.len(),
                    self.graph.edges.len(),
                    self.adapter
                )
            })
        };
        self.smoke_probe.as_mut().unwrap().complete(result);
        self.stop();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn validate_smoke_frame(&self) -> Result<(), String> {
        let positions = self.gpu.read_positions(self.graph.nodes.len())?;
        if positions.len() != self.graph.nodes.len()
            || positions.iter().flatten().any(|value| !value.is_finite())
        {
            return Err("GPU readback contained invalid node positions.".into());
        }
        if !positions
            .iter()
            .zip(&self.graph.nodes)
            .any(|(position, node)| {
                (position[0] - node.position[0]).abs() > 0.001
                    || (position[1] - node.position[1]).abs() > 0.001
            })
        {
            return Err("GPU physics did not move any nodes.".into());
        }
        let pixels = self.gpu.read_rgba()?;
        let (width, height) = self.gpu.dimensions();
        if pixels.len() != width as usize * height as usize * 4
            || !pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel != &pixels[..4])
        {
            return Err("GPU renderer returned an empty or uniform frame.".into());
        }
        Ok(())
    }

    fn parse_parameters(&mut self) -> Result<(), String> {
        let parameters: Value =
            serde_json::from_str(&self.parameters_text).map_err(|e| format!("Parameters: {e}"))?;
        if !parameters.is_object() {
            return Err("Parameters must be a JSON object.".into());
        }
        self.project.parameters = parameters;
        Ok(())
    }

    fn request(&mut self, intent: Intent) {
        self.confirm_example = None;
        self.error = None;
        if let Err(error) = self.parse_parameters() {
            self.error = Some(error);
            return;
        }
        let snapshot = self.project.clone();
        match RuleJob::start(
            snapshot.source.clone(),
            snapshot.parameters.clone(),
            snapshot.seed,
        ) {
            Ok(job) => {
                self.job = Some(job);
                self.pending_run = Some((intent, snapshot));
                self.status = "Checking your rules in an isolated JavaScript worker…".into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn start(&mut self, intent: Intent, snapshot: Project, plan: Plan) -> Result<(), String> {
        if intent == Intent::Validate {
            self.status = format!(
                "Rules valid · {} nodes · {} edges · {} explicit ticks",
                plan.node_count, plan.edge_count, plan.total_ticks
            );
            return Ok(());
        }
        if intent == Intent::Record {
            let metadata = json!({
                "app_version":env!("CARGO_PKG_VERSION"), "project":snapshot,
                "source_sha256":format!("{:x}", Sha256::digest(snapshot.source.as_bytes())),
                "adapter":self.adapter, "force_profile":"nodiform-exact-v1",
                "rule_api":crate::rules::RULE_API_VERSION,
                "forces":{"repulsion":64,"softening_squared":0.25,"rest_length":0,"gravity":0,"max_displacement":2,"step_policy":"min(1/120,0.5/max_incident_strength)"},
                "birth_placement":crate::model::BIRTH_PLACEMENT_VERSION,
                "ticks_per_frame":snapshot.ticks_per_frame,
                "planned_ticks":plan.total_ticks + u64::from(snapshot.tail_ticks),
                "planned_nodes":plan.node_count,"planned_edges":plan.edge_count,
                "frame_sampling":"after each ticks_per_frame ticks; final partial interval included",
                "cross_device_bit_identical":false
            });
            self.recorder_start = Some(RecorderStartJob::start(
                self.output_dir.clone(),
                snapshot.recording.clone(),
                metadata,
            )?);
            self.prepared_run = Some((snapshot, plan));
            self.status = "Checking the selected video encoder…".into();
            return Ok(());
        }
        self.begin_simulation(snapshot, plan);
        Ok(())
    }

    fn begin_simulation(&mut self, snapshot: Project, plan: Plan) {
        self.graph = Graph::new(snapshot.seed);
        self.gpu.sync_graph(&self.graph, true);
        self.timeline = Some(Timeline::new(plan, snapshot.tail_ticks));
        self.project = snapshot;
        self.pending_frame = None;
        self.finishing = false;
        self.paused = false;
        self.tick = 0;
        self.last_event = 0;
        self.inspected_position = None;
        self.components = 0;
        self.last_sample = Instant::now() - Duration::from_secs(1);
        self.status = if self.recorder.is_some() {
            "Recording every frame. Slower computation stretches wall time, not the movie."
        } else {
            "Preview running · no video files are being written."
        }
        .into();
    }

    fn stop(&mut self) {
        if let Some(job) = &mut self.job {
            job.cancel();
        }
        self.job = None;
        self.pending_run = None;
        self.recorder_start = None;
        self.prepared_run = None;
        self.timeline = None;
        self.paused = false;
        // An already rendered frame must reach the encoder before its channel is closed.
        if self.pending_frame.is_none() {
            self.finish_recording();
        }
        self.status = "Stopped. Finalising any recorded frames…".into();
    }

    fn finish_recording(&mut self) {
        if !self.finishing {
            let status = if self.error.is_some() {
                "error"
            } else if self.timeline.as_ref().is_some_and(Timeline::finished) {
                "completed"
            } else {
                "stopped"
            };
            let outcome = self.run_outcome(status);
            if let Some(recorder) = &mut self.recorder {
                if let Err(error) = recorder.set_run_outcome(outcome) {
                    self.error = Some(error);
                }
                recorder.finish();
                self.finishing = true;
            }
        }
    }

    fn run_outcome(&self, status: &str) -> Value {
        json!({"status":status,"actual_tick":self.tick,"events_consumed":self.last_event,
            "final_effective_timestep":self.gpu.effective_timestep(),
            "nodes":self.graph.nodes.len(),"edges":self.graph.edges.len(),"error":self.error})
    }

    fn render(&mut self) {
        let old = self.gpu.dimensions();
        self.gpu
            .render(self.project.recording.width, self.project.recording.height);
        if old != self.gpu.dimensions() {
            self.render_state
                .renderer
                .write()
                .update_egui_texture_from_wgpu_texture(
                    &self.render_state.device,
                    self.gpu.view(),
                    wgpu::FilterMode::Linear,
                    self.texture,
                );
        }
    }

    fn advance_frame(&mut self) -> Result<(), String> {
        self.inspected_position = None;
        let mut budget = self.project.ticks_per_frame;
        while budget > 0 {
            let Some(timeline) = &mut self.timeline else {
                break;
            };
            if timeline.finished() {
                break;
            }
            let (changed, ticks) = timeline.next(&mut self.graph, budget)?;
            self.tick = timeline.tick;
            if changed {
                self.gpu.sync_graph(&self.graph, false);
                self.components = self.graph.component_count();
            }
            if ticks > 0 {
                self.gpu.step(ticks);
                budget -= ticks;
            } else {
                break;
            }
        }
        // Mutations at exactly this sample's tick belong to this sample, not
        // a duplicate frame with the same tick on the next repaint.
        if let Some(timeline) = &mut self.timeline {
            let (changed, _) = timeline.next(&mut self.graph, 0)?;
            self.last_event = timeline.cursor;
            if changed {
                self.gpu.sync_graph(&self.graph, false);
                self.components = self.graph.component_count();
            }
        }
        self.render();
        if self.recorder.is_some() {
            self.pending_frame = Some(self.gpu.read_rgba()?);
        }
        Ok(())
    }

    fn pump(&mut self) {
        if let Some(result) = self
            .recorder_start
            .as_mut()
            .and_then(RecorderStartJob::poll)
        {
            self.recorder_start = None;
            match result {
                Ok(recorder) => {
                    self.recorder = Some(recorder);
                    if let Some((snapshot, plan)) = self.prepared_run.take() {
                        self.begin_simulation(snapshot, plan);
                    }
                }
                Err(error) => {
                    self.prepared_run = None;
                    self.error = Some(error);
                    self.status = "Recording did not start. Preview is still available.".into();
                }
            }
        }
        if let Some(result) = self.job.as_mut().and_then(RuleJob::poll) {
            self.job = None;
            if let Some((intent, snapshot)) = self.pending_run.take() {
                match result.and_then(|plan| self.start(intent, snapshot, plan)) {
                    Ok(()) => (),
                    Err(error) => {
                        self.error = Some(error);
                        self.status = "The run did not start.".into();
                    }
                }
            }
        }
        if let Some(result) = self.recorder.as_mut().and_then(Recorder::poll) {
            if let Err(error) = &result {
                self.error = Some(error.clone());
                let outcome = self.run_outcome("error");
                if !self.finishing {
                    if let Some(recorder) = &mut self.recorder {
                        let _ = recorder.set_run_outcome(outcome);
                    }
                }
            }
            self.recorder = None;
            self.finishing = false;
            match result {
                Ok(path) => self.status = format!("Video saved · {}", path.display()),
                Err(error) => {
                    self.error = Some(error);
                    self.timeline = None;
                    self.pending_frame = None;
                }
            }
        }
        if let Some(frame) = self.pending_frame.take() {
            if let Some(recorder) = &mut self.recorder {
                match recorder.try_frame(frame) {
                    Ok(returned) => self.pending_frame = returned,
                    Err(error) => {
                        self.error = Some(error);
                        self.timeline = None;
                        self.finish_recording();
                    }
                }
            }
        }
        if self.pending_frame.is_some() {
            return;
        }
        let finished = self.timeline.as_ref().is_none_or(Timeline::finished);
        if finished {
            self.finish_recording();
            if self.recorder.is_none() && self.timeline.is_some() {
                self.timeline = None;
                self.paused = false;
                if !self.status.starts_with("Video saved") {
                    self.status =
                        "Preview complete. Change the rules or seed to begin another experiment."
                            .into();
                }
            }
            return;
        }
        if !self.paused
            && !self.finishing
            && self.last_sample.elapsed()
                >= Duration::from_secs_f64(1.0 / f64::from(self.project.recording.fps))
        {
            if let Err(error) = self.advance_frame() {
                self.error = Some(error);
                self.stop();
            }
            self.last_sample = Instant::now();
        }
    }

    fn save(&mut self) {
        if let Err(error) = self.parse_parameters() {
            self.error = Some(error);
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("experiment.nodiform.json")
            .add_filter("Nodiform project", &["json"])
            .save_file()
        {
            let result = serde_json::to_vec_pretty(&self.project)
                .map_err(|e| e.to_string())
                .and_then(|data| std::fs::write(&path, data).map_err(|e| e.to_string()));
            match result {
                Ok(()) => {
                    self.status = format!("Project saved · {}", path.display());
                    self.project_path = Some(path);
                }
                Err(error) => self.error = Some(format!("Could not save project: {error}")),
            }
        }
    }

    fn open(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Nodiform project", &["json"])
            .pick_file()
        {
            let result = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| serde_json::from_str::<Project>(&text).map_err(|e| e.to_string()))
                .and_then(|project| {
                    validate_project(&project)?;
                    Ok(project)
                });
            match result {
                Ok(project) => {
                    self.parameters_text =
                        serde_json::to_string_pretty(&project.parameters).unwrap();
                    self.project = project;
                    self.project_path = Some(path);
                    self.status = "Project opened. Validate or preview when ready.".into();
                    self.error = None;
                }
                Err(error) => self.error = Some(format!("Could not open project: {error}")),
            }
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(16, 27, 34))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("◉").size(26.0).color(MINT));
                    ui.label(egui::RichText::new("Nodiform").size(24.0).strong());
                    ui.label(
                        egui::RichText::new("EMERGENT GRAPH LAB")
                            .size(10.0)
                            .color(MUTED),
                    );
                    ui.separator();
                    let idle = !self.busy();
                    if ui
                        .add_enabled(idle, egui::Button::new("Validate"))
                        .on_hover_text("Check rules and graph constraints without running physics")
                        .clicked()
                    {
                        self.request(Intent::Validate);
                    }
                    if ui
                        .add_enabled(idle, egui::Button::new("▷ Preview"))
                        .clicked()
                    {
                        self.request(Intent::Preview);
                    }
                    if ui
                        .add_enabled(
                            idle,
                            egui::Button::new(
                                egui::RichText::new("● Run & Record")
                                    .color(egui::Color32::from_rgb(12, 35, 29)),
                            )
                            .fill(MINT),
                        )
                        .clicked()
                    {
                        self.request(Intent::Record);
                    }
                    if ui
                        .add_enabled(
                            self.timeline.as_ref().is_some_and(|t| !t.finished())
                                && !self.finishing,
                            egui::Button::new(if self.paused { "Resume" } else { "Pause" }),
                        )
                        .clicked()
                    {
                        self.paused = !self.paused;
                    }
                    if ui
                        .add_enabled(
                            self.paused
                                && self.pending_frame.is_none()
                                && !self.finishing
                                && self.timeline.as_ref().is_some_and(|t| !t.finished()),
                            egui::Button::new("Step"),
                        )
                        .on_hover_text("Advance exactly one output-frame interval")
                        .clicked()
                    {
                        if let Err(error) = self.advance_frame() {
                            self.error = Some(error);
                            self.stop();
                        }
                    }
                    if ui
                        .add_enabled(self.busy(), egui::Button::new("Stop"))
                        .clicked()
                    {
                        self.stop();
                    }
                });
            });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        let enabled = !self.busy();
        egui::SidePanel::left("rules")
            .resizable(true)
            .default_width(510.0)
            .width_range(370.0..=900.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.heading("Your rules");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Save…"))
                            .clicked()
                        {
                            self.save();
                        }
                        if ui
                            .add_enabled(enabled, egui::Button::new("Open…"))
                            .clicked()
                        {
                            self.open();
                        }
                    });
                });
                ui.label(
                    egui::RichText::new("Define a process. Watch a structure emerge.").color(MUTED),
                );
                egui::ScrollArea::vertical()
                    .id_salt("experiment_settings")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Start from");
                                if ui.small_button("ABC permutations").clicked() {
                                    self.confirm_example = Some("abc");
                                }
                                if ui.small_button("Growing ring").clicked() {
                                    self.confirm_example = Some("ring");
                                }
                            });
                            egui::CollapsingHeader::new("Experiment parameters")
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Seed");
                                        ui.add(egui::DragValue::new(&mut self.project.seed));
                                        ui.label("Settle ticks");
                                        ui.add(
                                            egui::DragValue::new(&mut self.project.tail_ticks)
                                                .range(0..=10_000_000),
                                        );
                                    });
                                    self.easy_parameters(ui);
                                    egui::CollapsingHeader::new("All parameters · JSON").show(
                                        ui,
                                        |ui| {
                                            ui.add(
                                                egui::TextEdit::multiline(
                                                    &mut self.parameters_text,
                                                )
                                                .font(egui::TextStyle::Monospace)
                                                .desired_rows(6)
                                                .desired_width(f32::INFINITY)
                                                .code_editor(),
                                            );
                                        },
                                    );
                                });
                            egui::CollapsingHeader::new("Recording & timing").show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Resolution");
                                    egui::ComboBox::from_id_salt("resolution")
                                        .selected_text(format!(
                                            "{} × {}",
                                            self.project.recording.width,
                                            self.project.recording.height
                                        ))
                                        .show_ui(ui, |ui| {
                                            for (w, h) in [(1280, 720), (1920, 1080), (3840, 2160)]
                                            {
                                                if ui
                                                    .selectable_label(
                                                        self.project.recording.width == w,
                                                        format!("{w} × {h}"),
                                                    )
                                                    .clicked()
                                                {
                                                    self.project.recording.width = w;
                                                    self.project.recording.height = h;
                                                }
                                            }
                                        });
                                });
                                ui.horizontal(|ui| {
                                    ui.label("FPS");
                                    ui.add(
                                        egui::DragValue::new(&mut self.project.recording.fps)
                                            .range(1..=120),
                                    );
                                    ui.label("Ticks/frame");
                                    ui.add(
                                        egui::DragValue::new(&mut self.project.ticks_per_frame)
                                            .range(1..=240),
                                    );
                                });
                                egui::ComboBox::from_id_salt("codec")
                                    .selected_text(&self.project.recording.codec)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.project.recording.codec,
                                            "libx264".into(),
                                            "H.264 · CPU, high quality",
                                        );
                                        ui.selectable_value(
                                            &mut self.project.recording.codec,
                                            "h264_nvenc".into(),
                                            "H.264 · NVIDIA NVENC",
                                        );
                                    });
                                if ui.button("Choose video folder…").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                        self.output_dir = path;
                                    }
                                }
                                ui.label(
                                    egui::RichText::new(self.output_dir.display().to_string())
                                        .small()
                                        .color(MUTED),
                                );
                                ui.label(
                                    egui::RichText::new(
                                        "MKV video + reproducible rule manifest. FFmpeg required.",
                                    )
                                    .small(),
                                );
                            });
                        });
                    });
                ui.separator();
                editor::show(ui, &mut self.project.source, enabled);
            });
    }

    fn easy_parameters(&mut self, ui: &mut egui::Ui) {
        let Ok(mut value) = serde_json::from_str::<Value>(&self.parameters_text) else {
            return;
        };
        let mut changed = false;
        if let Some(alphabet) = value
            .get("alphabet")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            let mut alphabet = alphabet;
            ui.horizontal(|ui| {
                ui.label("Alphabet");
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut alphabet).desired_width(90.0))
                    .changed();
            });
            value["alphabet"] = json!(alphabet);
        }
        if let Some(length) = value.get("maxLength").and_then(Value::as_u64) {
            let mut length = length;
            ui.horizontal(|ui| {
                ui.label("Maximum length");
                changed |= ui
                    .add(egui::DragValue::new(&mut length).range(1..=10))
                    .changed();
            });
            value["maxLength"] = json!(length);
        }
        if let Some(repeat) = value.get("repetitions").and_then(Value::as_bool) {
            let mut repeat = repeat;
            changed |= ui
                .checkbox(&mut repeat, "Allow repeated letters, such as AA")
                .changed();
            value["repetitions"] = json!(repeat);
        }
        if let Some(order) = value.get("order").and_then(Value::as_str) {
            let mut order = order.to_string();
            egui::ComboBox::from_id_salt("order")
                .selected_text(&order)
                .show_ui(ui, |ui| {
                    for option in ["lexicographic", "reverse", "shuffle"] {
                        changed |= ui
                            .selectable_value(&mut order, option.into(), option)
                            .changed();
                    }
                });
            value["order"] = json!(order);
        }
        if changed {
            self.parameters_text = serde_json::to_string_pretty(&value).unwrap();
        }
    }

    fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::from_rgb(10, 18, 24)).inner_margin(20)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("OBSERVATORY").color(MUTED).size(11.0));
                ui.label(if self.paused { "Paused" } else if self.busy() { "● Live" } else { "Ready" });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(egui::RichText::new("2D  /  AUTO FIT  /  NO GRAVITY").color(MUTED).size(10.0)); });
            });
            let available = egui::vec2(ui.available_width(), (ui.available_height() - 135.0).max(100.0));
            let aspect = self.gpu.dimensions().0 as f32 / self.gpu.dimensions().1 as f32;
            let size = if available.x / available.y > aspect { egui::vec2(available.y * aspect, available.y) } else { egui::vec2(available.x, available.x / aspect) };
            ui.vertical_centered(|ui| { ui.add(egui::Image::new((self.texture, size))); });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for (number, label) in [(self.graph.nodes.len() as u64,"nodes"),(self.graph.edges.len() as u64,"edges"),(self.tick,"ticks"),(self.components as u64,"components")] {
                    ui.label(egui::RichText::new(number.to_string()).size(21.0).color(MINT)); ui.label(egui::RichText::new(label).color(MUTED)); ui.add_space(12.0);
                }
            });
            if self.components > 1 { ui.label(egui::RichText::new("Disconnected components can drift apart: repulsion is active and gravity is deliberately absent.").small().color(egui::Color32::from_rgb(232, 186, 117))); }
            if !self.graph.nodes.is_empty() {
                egui::CollapsingHeader::new("Inspect a node").show(ui, |ui| {
                    self.inspector = self.inspector.min(self.graph.nodes.len()-1);
                    ui.horizontal(|ui| {
                        ui.label("Node index");
                        if ui.add(egui::DragValue::new(&mut self.inspector).range(0..=self.graph.nodes.len()-1)).changed() { self.inspected_position = None; }
                        let node = &self.graph.nodes[self.inspector];
                        ui.label(format!("{} · radius {} · {} connections", node.label, node.radius, self.graph.edges.iter().filter(|e| e.source == self.inspector || e.target == self.inspector).count()));
                    });
                    if ui.add_enabled(self.paused || !self.busy(), egui::Button::new("Read current position")).clicked() {
                        match self.gpu.read_positions(self.graph.nodes.len()) {
                            Ok(positions) => self.inspected_position = positions.get(self.inspector).copied(),
                            Err(error) => self.error = Some(error),
                        }
                    }
                    if let Some([x,y]) = self.inspected_position { ui.label(format!("x {x:.3}   y {y:.3}")); }
                });
            }
        });
    }
}

impl eframe::App for NodiformApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        self.toolbar(ctx);
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(6.0);
            if let Some(error) = &self.error {
                egui::ScrollArea::vertical()
                    .id_salt("error_details")
                    .max_height(90.0)
                    .show(ui, |ui| {
                        ui.colored_label(egui::Color32::from_rgb(255, 154, 145), error);
                    });
            }
            ui.label(&self.status);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.adapter).small().color(MUTED));
                if let Some(recorder) = &self.recorder {
                    ui.label(format!(
                        "{} frames {}",
                        recorder.frames(),
                        if self.finishing {
                            "· finalising"
                        } else if self.pending_frame.is_some() {
                            "· encoder backpressure"
                        } else {
                            "· captured"
                        }
                    ))
                    .on_hover_text(recorder.directory().display().to_string());
                }
            });
            ui.add_space(6.0);
        });
        self.side_panel(ctx);
        self.canvas(ctx);
        if let Some(example) = self.confirm_example {
            egui::Window::new("Replace the current rules?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Save first if you want to keep your current edits.");
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.confirm_example = None;
                        }
                        if ui.button("Load example").clicked() {
                            self.project = Project::default();
                            if example == "ring" {
                                self.project.source = include_str!("../examples/ring.js").into();
                                self.project.parameters = json!({"count":80,"ticksPerNode":12});
                            }
                            self.parameters_text =
                                serde_json::to_string_pretty(&self.project.parameters).unwrap();
                            self.project_path = None;
                            self.confirm_example = None;
                        }
                    });
                });
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
        // Keep the event loop alive while FFmpeg drains on close.
        if ctx.input(|i| i.viewport().close_requested()) && self.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.stop();
            self.status =
                "Finishing safely. Close the window again when the video has been saved.".into();
        }
        self.check_smoke_test(ctx);
    }
}

fn validate_project(project: &Project) -> Result<(), String> {
    if project.schema != 1 {
        return Err("Unsupported project schema.".into());
    }
    if !project.parameters.is_object() {
        return Err("Parameters must be an object.".into());
    }
    if project.source.len() > 1_048_576 {
        return Err("Rule source exceeds 1 MiB.".into());
    }
    if !(1..=240).contains(&project.ticks_per_frame) || !(1..=120).contains(&project.recording.fps)
    {
        return Err("Invalid timing settings.".into());
    }
    if ![(1280, 720), (1920, 1080), (3840, 2160)]
        .contains(&(project.recording.width, project.recording.height))
    {
        return Err("Unsupported recording resolution.".into());
    }
    if !["libx264", "h264_nvenc"].contains(&project.recording.codec.as_str()) {
        return Err("Unsupported encoder.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_round_trip() {
        let original = Project::default();
        let saved = serde_json::to_string(&original).unwrap();
        let restored: Project = serde_json::from_str(&saved).unwrap();
        validate_project(&restored).unwrap();
        assert_eq!(original.source, restored.source);
        assert_eq!(original.parameters, restored.parameters);
    }
    #[test]
    fn reject_invalid_timing() {
        let project = Project {
            ticks_per_frame: 0,
            ..Project::default()
        };
        assert!(validate_project(&project).is_err());
    }
}
