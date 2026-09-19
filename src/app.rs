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

mod playback;
mod ui;

#[derive(Clone, Serialize, Deserialize)]
struct Project {
    schema: u32,
    source: String,
    parameters: Value,
    seed: u32,
    ticks_per_frame: u32,
    tail_ticks: u32,
    recording: RecordingConfig,
    #[serde(default)]
    size_by_connections: bool,
    #[serde(default = "default_edge_width")]
    edge_width: f32,
}

fn default_edge_width() -> f32 {
    1.0
}

impl Default for Project {
    fn default() -> Self {
        Self {
            schema: 1,
            source: include_str!("../examples/starter.js").into(),
            parameters: json!({}),
            seed: 42,
            ticks_per_frame: 4,
            tail_ticks: 240,
            recording: RecordingConfig {
                width: 1280,
                height: 720,
                fps: 60,
                codec: "libx264".into(),
            },
            size_by_connections: false,
            edge_width: default_edge_width(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Intent {
    Validate,
    Preview,
    Record,
}

#[derive(Clone, Copy, PartialEq)]
enum EditorTab {
    Rules,
    Inputs,
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
    progress: Option<playback::RunProgress>,
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
    preview_clock: playback::PreviewClock,
    preview_dirty: bool,
    preview_pixels: (u32, u32),
    adapter: String,
    inspector: usize,
    inspected_position: Option<[f32; 2]>,
    confirm_example: Option<&'static str>,
    smoke_probe: Option<SmokeProbe>,
    editor_tab: EditorTab,
    show_settings: bool,
    theme_preference: egui::ThemePreference,
    show_inspector: bool,
    smoke_frame_checked: bool,
    smoke_idle_since: Option<Instant>,
}

impl NodiformApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        smoke_probe: Option<SmokeProbe>,
    ) -> Result<Self, String> {
        let theme_preference = if smoke_probe.is_some() {
            match std::env::var("NODIFORM_SMOKE_THEME").as_deref() {
                Ok("dark") => egui::ThemePreference::Dark,
                Ok("light") => egui::ThemePreference::Light,
                _ => egui::ThemePreference::System,
            }
        } else {
            crate::theme::restore(cc.storage)
        };
        crate::theme::configure(&cc.egui_ctx, theme_preference);
        if smoke_probe.is_some() && std::env::var("NODIFORM_SMOKE_HIDPI").as_deref() == Ok("1") {
            cc.egui_ctx.set_pixels_per_point(2.0);
        }
        let render_state = cc
            .wgpu_render_state
            .clone()
            .ok_or("Nodiform requires a compute-capable GPU")?;
        let info = render_state.adapter.get_info();
        let adapter = format!("{} · {:?}", info.name, info.backend);
        let mut gpu = GpuGraph::new(
            Arc::new(render_state.device.clone()),
            Arc::new(render_state.queue.clone()),
        )?;
        let graph = Graph::new(42);
        gpu.sync_graph(&graph, true)?;
        gpu.render_preview(1, 1);
        let texture = render_state.renderer.write().register_native_texture(
            &render_state.device,
            gpu.preview_view(),
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
            progress: None,
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
            status: "Your rules, your structure. Edit the script, then choose Preview.".into(),
            error: None,
            last_sample: Instant::now(),
            preview_clock: playback::PreviewClock::default(),
            preview_dirty: true,
            preview_pixels: (1, 1),
            adapter,
            inspector: 0,
            inspected_position: None,
            confirm_example: None,
            smoke_probe,
            editor_tab: EditorTab::Rules,
            show_settings: false,
            theme_preference,
            show_inspector: false,
            smoke_frame_checked: false,
            smoke_idle_since: None,
        };
        if app.smoke_probe.is_some() {
            if let Ok(example) = std::env::var("NODIFORM_SMOKE_EXAMPLE") {
                app.load_example(&example);
            }
            if let Ok(parameters) = std::env::var("NODIFORM_SMOKE_PARAMETERS") {
                // Use the same input buffer and validation path as the editor.
                app.parameters_text = parameters;
            }
            if std::env::var("NODIFORM_SMOKE_TAB").as_deref() == Ok("inputs") {
                app.editor_tab = EditorTab::Inputs;
            }
            app.show_settings = std::env::var("NODIFORM_SMOKE_SETTINGS").as_deref() == Ok("1");
            if std::env::var("NODIFORM_SMOKE_DEGREE").as_deref() == Ok("1") {
                app.project.size_by_connections = true;
            }
            if let Ok(width) = std::env::var("NODIFORM_SMOKE_EDGE_WIDTH") {
                app.project.edge_width = width
                    .parse()
                    .map_err(|_| "Invalid smoke-test edge thickness")?;
                validate_project(&app.project)?;
            }
            // Exercise the actual child process, timeline and rendering path.
            // Recording smoke tests use an explicit or temporary output folder.
            let intent = if std::env::var("NODIFORM_SMOKE_INTENT").as_deref() == Ok("record") {
                app.output_dir = std::env::var_os("NODIFORM_SMOKE_OUTPUT_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::temp_dir()
                            .join(format!("nodiform-smoke-recording-{}", std::process::id()))
                    });
                Intent::Record
            } else {
                Intent::Preview
            };
            app.request(intent);
        }
        Ok(app)
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
        let wait_for_completion = std::env::var("NODIFORM_SMOKE_COMPLETE").as_deref() == Ok("1")
            && !self
                .progress
                .as_ref()
                .is_some_and(|progress| progress.state == playback::RunState::Complete);
        if (!ready || wait_for_completion) && self.error.is_none() {
            ctx.request_repaint_after(Duration::from_millis(8));
            return;
        }
        let frames = probe.frames;
        if self.error.is_none() && !self.smoke_frame_checked {
            if let Err(error) = self.validate_smoke_frame() {
                self.error = Some(error);
            }
            self.smoke_frame_checked = true;
            // Keep the simulated graph, then let idle controls and window
            // animations settle before capturing the actual interface.
            self.stop();
            self.smoke_idle_since = Some(Instant::now());
            self.status = "Preview checked. Your experiment is ready to edit.".into();
            ctx.request_repaint();
            return;
        }
        if self.error.is_none() {
            if self.recorder.is_some() {
                // Do not report a successful recording smoke test until FFmpeg
                // has encoded every queued frame and committed the manifest.
                ctx.request_repaint_after(Duration::from_millis(16));
                return;
            }
            if self
                .smoke_idle_since
                .is_some_and(|since| since.elapsed() < Duration::from_millis(300))
            {
                ctx.request_repaint_after(Duration::from_millis(16));
                return;
            }
            match self.smoke_probe.as_mut().unwrap().capture(ctx) {
                Ok(false) => {
                    ctx.request_repaint();
                    return;
                }
                Ok(true) => {}
                Err(error) => self.error = Some(error),
            }
        }
        let result = self.error.clone().map_or_else(
            || {
                Ok(format!(
                    "{} GUI updates, {} ticks, {} nodes, {} edges; preview {} × {} physical pixels; progress {:.1}%; {}",
                    frames,
                    self.tick,
                    self.graph.nodes.len(),
                    self.graph.edges.len(),
                    self.preview_pixels.0,
                    self.preview_pixels.1,
                    self.progress.as_ref().map_or(0.0, |progress| progress.fraction() * 100.0),
                    self.adapter
                ))
            },
            Err,
        );
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
        let pixels = self.gpu.read_preview_rgba()?;
        let (width, height) = self.gpu.preview_dimensions();
        if (width, height) != self.preview_pixels {
            return Err("Preview texture does not match the canvas's physical pixels.".into());
        }
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
                if intent != Intent::Validate {
                    self.progress = None;
                }
                self.job = Some(job);
                self.pending_run = Some((intent, snapshot));
                self.status = "Checking your rules in an isolated JavaScript worker…".into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn start(&mut self, intent: Intent, snapshot: Project, plan: Plan) -> Result<(), String> {
        self.gpu
            .validate_capacity(plan.node_count, plan.edge_count)?;
        let planned_ticks = plan
            .total_ticks
            .checked_add(u64::from(snapshot.tail_ticks))
            .ok_or("Total simulation duration including settling ticks overflowed")?;
        if intent == Intent::Validate {
            self.status = format!(
                "Rules valid · {} nodes · {} edges · {} explicit ticks",
                plan.node_count, plan.edge_count, plan.total_ticks
            );
            return Ok(());
        }
        self.progress = Some(playback::RunProgress::new(
            intent == Intent::Record,
            planned_ticks,
        ));
        if intent == Intent::Record {
            let effective_parameters = crate::experiment::merge_defaults(
                &crate::experiment::parse_controls(&snapshot.source)?,
                &snapshot.parameters,
            )?;
            let metadata = json!({
                "app_version":env!("CARGO_PKG_VERSION"), "project":snapshot,
                "source_sha256":format!("{:x}", Sha256::digest(snapshot.source.as_bytes())),
                "adapter":self.adapter, "force_profile":crate::model::FORCE_VERSION,
                "rule_api":crate::rules::RULE_API_VERSION,
                "effective_parameters": effective_parameters,
                "node_size_rule":if snapshot.size_by_connections { "obsidian-global-sqrt-uncapped-v1" } else { "rule-radius" },
                "edge_width_multiplier":snapshot.edge_width,
                "forces":{"version":crate::model::FORCE_VERSION,"repulsion":crate::model::REPULSION,"default_edge_strength":crate::model::DEFAULT_EDGE_STRENGTH,"softening_squared":crate::model::SOFTENING_SQUARED,"rest_length":0,"gravity":0,"momentum_retention":crate::model::MOMENTUM_RETENTION,"max_displacement":crate::model::MAX_DISPLACEMENT,"base_timestep":crate::model::BASE_TIMESTEP,"step_policy":"min(base_timestep,0.5/max_incident_strength)"},
                "birth_placement":crate::model::BIRTH_PLACEMENT_VERSION,
                "ticks_per_frame":snapshot.ticks_per_frame,
                "planned_ticks":planned_ticks,
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
        self.begin_simulation(snapshot, plan)
    }

    fn begin_simulation(&mut self, snapshot: Project, plan: Plan) -> Result<(), String> {
        self.graph = Graph::new(snapshot.seed);
        self.gpu.set_degree_sizing(snapshot.size_by_connections);
        self.gpu.set_edge_width(snapshot.edge_width);
        self.gpu.sync_graph(&self.graph, true)?;
        self.timeline = Some(Timeline::new(plan, snapshot.tail_ticks));
        if let (Some(progress), Some(timeline)) = (&mut self.progress, &self.timeline) {
            progress.observe(timeline);
        }
        self.project = snapshot;
        self.pending_frame = None;
        self.finishing = false;
        self.paused = false;
        self.tick = 0;
        self.last_event = 0;
        self.inspected_position = None;
        self.components = 0;
        self.reset_preview_clock();
        self.preview_dirty = true;
        self.status = if self.recorder.is_some() {
            "Recording every frame. Slower computation stretches wall time, not the movie."
        } else {
            "Preview running · no video files are being written."
        }
        .into();
        Ok(())
    }

    fn stop(&mut self) {
        let completed_simulation =
            self.error.is_none() && self.timeline.as_ref().is_some_and(Timeline::finished);
        if !completed_simulation {
            if let Some(progress) = &mut self.progress {
                progress.stop(self.error.is_some());
            }
        }
        if let Some(job) = &mut self.job {
            job.cancel();
        }
        self.job = None;
        self.pending_run = None;
        self.recorder_start = None;
        self.prepared_run = None;
        // The encoder may still be draining a fully simulated run. Retain its
        // finished timeline so closing the window cannot relabel it as stopped.
        if !completed_simulation {
            self.timeline = None;
        }
        self.paused = false;
        self.reset_preview_clock();
        // An already rendered frame must reach the encoder before its channel is closed.
        if self.pending_frame.is_none() {
            self.finish_recording();
        }
        self.status = if completed_simulation && self.recorder.is_some() {
            "Finalising recorded video…"
        } else if completed_simulation {
            "Preview complete."
        } else if self.recorder.is_some() {
            "Stopped. Finalising recorded frames…"
        } else {
            "Preview stopped."
        }
        .into();
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
                    if let Some(progress) = &mut self.progress {
                        progress.stop(true);
                    }
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

    fn render_preview(&mut self, canvas_size: egui::Vec2, pixels_per_point: f32) {
        let size = playback::preview_dimensions(
            canvas_size,
            pixels_per_point,
            self.render_state.device.limits().max_texture_dimension_2d,
        );
        self.preview_pixels = size;
        let old = self.gpu.preview_dimensions();
        if !self.preview_dirty && old == size {
            return;
        }
        self.gpu.render_preview(size.0, size.1);
        self.preview_dirty = false;
        if old != self.gpu.preview_dimensions() {
            self.render_state
                .renderer
                .write()
                .update_egui_texture_from_wgpu_texture(
                    &self.render_state.device,
                    self.gpu.preview_view(),
                    wgpu::FilterMode::Linear,
                    self.texture,
                );
        }
    }

    fn reset_preview_clock(&mut self) {
        self.last_sample = Instant::now();
        self.preview_clock.reset();
    }

    fn advance_ticks(&mut self, mut budget: u32) -> Result<(), String> {
        self.inspected_position = None;
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
                self.gpu.sync_graph(&self.graph, false)?;
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
                self.gpu.sync_graph(&self.graph, false)?;
                self.components = self.graph.component_count();
            }
            if let Some(progress) = &mut self.progress {
                progress.observe(timeline);
            }
        }
        self.preview_dirty = true;
        Ok(())
    }

    fn advance_frame(&mut self) -> Result<(), String> {
        self.advance_ticks(self.project.ticks_per_frame)?;
        if self.recorder.is_some() {
            self.gpu
                .render(self.project.recording.width, self.project.recording.height);
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
                        if let Err(error) = self.begin_simulation(snapshot, plan) {
                            self.error = Some(error);
                            self.stop();
                        }
                    }
                }
                Err(error) => {
                    self.prepared_run = None;
                    if let Some(progress) = &mut self.progress {
                        progress.stop(true);
                    }
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
                        if intent != Intent::Validate {
                            if let Some(progress) = &mut self.progress {
                                progress.stop(true);
                            }
                        }
                        self.error = Some(error);
                        self.status = "The run did not start.".into();
                    }
                }
            }
        }
        if let Some(result) = self.recorder.as_mut().and_then(Recorder::poll) {
            if let Some(progress) = &mut self.progress {
                progress.encoding_finished(result.is_ok());
            }
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
                        if let Some(progress) = &mut self.progress {
                            progress.stop(true);
                        }
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
        if self.paused || self.finishing {
            self.reset_preview_clock();
            return;
        }
        // Recording samples every fixed interval and honours encoder pressure.
        // Preview instead follows elapsed time at the monitor's refresh rate.
        let result = if self.recorder.is_some() {
            self.advance_frame()
        } else {
            let now = Instant::now();
            let elapsed = now.duration_since(self.last_sample);
            self.last_sample = now;
            let ticks = self.preview_clock.ticks(
                elapsed,
                self.project.recording.fps * self.project.ticks_per_frame,
            );
            if ticks == 0 {
                return;
            }
            self.advance_ticks(ticks)
        };
        if let Err(error) = result {
            self.error = Some(error);
            self.stop();
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
                    self.gpu.set_degree_sizing(self.project.size_by_connections);
                    self.gpu.set_edge_width(self.project.edge_width);
                    self.progress = None;
                    self.preview_dirty = true;
                    self.project_path = Some(path);
                    self.status = "Project opened. Validate or preview when ready.".into();
                    self.error = None;
                }
                Err(error) => self.error = Some(format!("Could not open project: {error}")),
            }
        }
    }
}

impl eframe::App for NodiformApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.smoke_probe.is_none() {
            crate::theme::save(storage, self.theme_preference);
        }
    }

    fn persist_egui_memory(&self) -> bool {
        self.smoke_probe.is_none()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        if !self.busy()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter))
        {
            self.request(Intent::Preview);
        }
        self.toolbar(ctx);
        self.status_bar(ctx);
        self.side_panel(ctx);
        self.canvas(ctx);
        self.settings(ctx);
        self.inspector_window(ctx);
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
                            self.load_example(example);
                        }
                    });
                });
        }
        if self.timeline.as_ref().is_some_and(|time| !time.finished())
            && !self.paused
            && !self.finishing
        {
            ctx.request_repaint();
        } else if self.busy() {
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
    if !project.edge_width.is_finite() || !(0.25..=8.0).contains(&project.edge_width) {
        return Err("Edge thickness must be between 0.25× and 8×.".into());
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
        let original = Project {
            size_by_connections: true,
            edge_width: 3.25,
            ..Project::default()
        };
        let saved = serde_json::to_string(&original).unwrap();
        let restored: Project = serde_json::from_str(&saved).unwrap();
        validate_project(&restored).unwrap();
        assert_eq!(original.source, restored.source);
        assert_eq!(original.parameters, restored.parameters);
        assert!(restored.size_by_connections);
        assert_eq!(restored.edge_width, 3.25);
    }
    #[test]
    fn older_projects_keep_fixed_rule_radii_by_default() {
        let mut value = serde_json::to_value(Project::default()).unwrap();
        value.as_object_mut().unwrap().remove("size_by_connections");
        value.as_object_mut().unwrap().remove("edge_width");
        let restored: Project = serde_json::from_value(value).unwrap();
        assert!(!restored.size_by_connections);
        assert_eq!(restored.edge_width, 1.0);
        validate_project(&restored).unwrap();
    }
    #[test]
    fn reject_invalid_timing() {
        let project = Project {
            ticks_per_frame: 0,
            ..Project::default()
        };
        assert!(validate_project(&project).is_err());
    }
    #[test]
    fn reject_invalid_edge_thickness() {
        for width in [0.0, 0.24, 8.01, f32::NAN, f32::INFINITY] {
            let project = Project {
                edge_width: width,
                ..Project::default()
            };
            assert!(validate_project(&project).is_err());
        }
        for width in [0.25, 1.0, 8.0] {
            let project = Project {
                edge_width: width,
                ..Project::default()
            };
            validate_project(&project).unwrap();
        }
    }
}
