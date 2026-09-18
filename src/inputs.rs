//! A source-driven form. The application has no knowledge of experiment-specific keys.
use crate::experiment::{
    parse_controls, validate_parameters, validate_value, ControlKind, ControlSpec,
};
use eframe::egui::{self, Color32, RichText};
use serde_json::Value;

const TEXT: Color32 = Color32::from_rgb(32, 37, 54);
const SECONDARY: Color32 = Color32::from_rgb(105, 114, 132);
const BLUE: Color32 = Color32::from_rgb(0, 122, 255);
const ERROR: Color32 = Color32::from_rgb(183, 49, 57);

/// Optional controls are declared in the script. Merely viewing a default never
/// changes the stored JSON; changing a field preserves every other parameter.
pub fn show(ui: &mut egui::Ui, source: &str, parameters_text: &mut String, enabled: bool) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        ui.visuals_mut().override_text_color = Some(TEXT);
        egui::ScrollArea::vertical()
            .id_salt("script-inputs-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(RichText::new("Experiment inputs").size(19.0).strong());
                secondary(ui, "Optional controls declared by your script. Values can also live directly in the code.");
                ui.add_space(6.0);

                let controls = parse_controls(source);
                let parsed = serde_json::from_str::<Value>(parameters_text);
                let reveal_json;
                match (&controls, parsed) {
                    (Ok(controls), Ok(mut values)) if values.is_object() => {
                        if controls.is_empty() {
                            card(ui, |ui| {
                                ui.label(RichText::new("Everything starts in the rules").strong());
                                secondary(ui, "This script does not declare any inputs. Define values in the editor, or use Insert → Optional controls to expose the values you want to adjust here.");
                            });
                        }
                        reveal_json = validate_parameters(controls, &values).is_err();
                        let mut changed = false;
                        // Source is part of the ID so switching scripts never reuses an
                        // unfinished text or JSON draft from a different experiment.
                        ui.push_id(source, |ui| {
                            ui.add_enabled_ui(enabled, |ui| {
                                for (name, spec) in controls {
                                    let explicit = values.get(name).is_some();
                                    let current = values.get(name).unwrap_or(&spec.default);
                                    let replacement = ui.push_id(name, |ui| {
                                        control_card(ui, name, spec, current, explicit)
                                    }).inner;
                                    if let Some(value) = replacement {
                                        values.as_object_mut().unwrap().insert(name.clone(), value);
                                        changed = true;
                                    }
                                    ui.add_space(2.0);
                                }
                            });
                        });
                        if changed {
                            *parameters_text = serde_json::to_string_pretty(&values).unwrap();
                        }
                    }
                    (Err(error), _) => {
                        issue(ui, "Controls could not be read", error);
                        secondary(ui, "Correct the leading @controls comment in the Rules tab. Your saved inputs remain intact.");
                        reveal_json = true;
                    }
                    (_, Err(error)) => {
                        issue(ui, "Saved inputs need attention", &format!("Invalid JSON: {error}"));
                        secondary(ui, "Correct the JSON below to restore the input controls.");
                        reveal_json = true;
                    }
                    (_, Ok(_)) => {
                        issue(ui, "Saved inputs need attention", "Inputs must be a JSON object, for example {}. Correct the JSON below.");
                        reveal_json = true;
                    }
                }
                ui.add_space(12.0);
                let mut raw = egui::CollapsingHeader::new("Advanced · Input JSON")
                    .id_salt("script-inputs-json");
                if reveal_json {
                    raw = raw.open(Some(true));
                }
                raw.show(ui, |ui| {
                    secondary(ui, "All stored inputs, including custom arrays and objects. Keys without a control remain available to the script.");
                    ui.add_space(4.0);
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.add(egui::TextEdit::multiline(parameters_text)
                            .id_salt("input-json-text")
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(9));
                    });
                });
            });
    });
}

fn control_card(
    ui: &mut egui::Ui,
    name: &str,
    spec: &ControlSpec,
    current: &Value,
    explicit: bool,
) -> Option<Value> {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&spec.label).size(14.0).strong());
            if !explicit {
                ui.label(RichText::new("Default").size(11.0).color(SECONDARY));
            }
        });
        if let Some(description) = &spec.description {
            secondary(ui, description);
        }
        let validation = validate_value(spec, current);
        let mut replacement = if compatible_type(spec.kind, current) {
            field(ui, spec, current)
        } else {
            secondary(ui, "The saved value has a different type. Edit Input JSON below, or explicitly use the script's default.");
            ui.button("Use default").clicked().then(|| spec.default.clone())
        };
        if let Err(error) = validation {
            ui.label(RichText::new(format!("{name}: {error}")).size(12.0).color(ERROR));
        }
        if explicit {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("params.{name}")).monospace().size(11.0).color(SECONDARY));
                // This is an explicit action. No field is reset while rendering.
                if ui.small_button("Reset value").on_hover_text("Use the default declared in this script.").clicked() {
                    replacement = Some(spec.default.clone());
                }
            });
        }
        replacement
    }).inner
}

fn compatible_type(kind: ControlKind, value: &Value) -> bool {
    match kind {
        ControlKind::Integer | ControlKind::Number => value.is_number(),
        ControlKind::Boolean => value.is_boolean(),
        ControlKind::Text | ControlKind::Color | ControlKind::Select => value.is_string(),
        ControlKind::Json => true,
    }
}

fn field(ui: &mut egui::Ui, spec: &ControlSpec, current: &Value) -> Option<Value> {
    match spec.kind {
        ControlKind::Integer | ControlKind::Number => {
            let mut number = current.as_f64().unwrap();
            let step = spec.step.unwrap_or(if spec.kind == ControlKind::Integer {
                1.0
            } else {
                0.1
            });
            let mut widget = egui::DragValue::new(&mut number)
                .speed(step)
                .range(spec.min.unwrap_or(-1_000_000.0)..=spec.max.unwrap_or(1_000_000.0))
                .clamp_existing_to_range(false);
            if spec.kind == ControlKind::Integer {
                widget = widget.max_decimals(0);
            }
            if ui.add(widget).changed() {
                if spec.kind == ControlKind::Integer {
                    Some(Value::from(number.round() as i64))
                } else {
                    serde_json::Number::from_f64(number).map(Value::Number)
                }
            } else {
                None
            }
        }
        ControlKind::Boolean => {
            let mut value = current.as_bool().unwrap();
            let response = ui
                .horizontal(|ui| {
                    let response = switch(ui, &mut value, &spec.label);
                    secondary(ui, if value { "On" } else { "Off" });
                    response
                })
                .inner;
            response.changed().then_some(Value::Bool(value))
        }
        ControlKind::Text => {
            let mut value = current.as_str().unwrap().to_owned();
            ui.add(egui::TextEdit::singleline(&mut value).desired_width(f32::INFINITY))
                .changed()
                .then_some(Value::String(value))
        }
        ControlKind::Color => {
            let mut value = current.as_str().unwrap().to_owned();
            let mut changed = false;
            ui.horizontal(|ui| {
                if let Ok(color) = crate::model::parse_color(&value) {
                    let mut channels = color.map(|channel| (channel * 255.0).round() as u8);
                    if ui
                        .color_edit_button_srgba_unmultiplied(&mut channels)
                        .changed()
                    {
                        value = format!(
                            "#{:02x}{:02x}{:02x}{:02x}",
                            channels[0], channels[1], channels[2], channels[3]
                        );
                        changed = true;
                    }
                }
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut value)
                            .hint_text("#RRGGBB or #RRGGBBAA")
                            .desired_width(ui.available_width()),
                    )
                    .changed();
            });
            changed.then_some(Value::String(value))
        }
        ControlKind::Select => {
            let mut value = current.as_str().unwrap().to_owned();
            let mut changed = false;
            egui::ComboBox::from_id_salt("choice")
                .selected_text(&value)
                .width(ui.available_width().min(320.0))
                .show_ui(ui, |ui| {
                    for option in &spec.options {
                        changed |= ui
                            .selectable_value(&mut value, option.clone(), option)
                            .changed();
                    }
                });
            changed.then_some(Value::String(value))
        }
        ControlKind::Json => json_field(ui, current),
    }
}

#[derive(Clone)]
struct JsonDraft {
    observed: String,
    text: String,
}

fn json_field(ui: &mut egui::Ui, current: &Value) -> Option<Value> {
    let id = ui.make_persistent_id("json-draft");
    let stored = serde_json::to_string_pretty(current).unwrap();
    let mut draft = ui
        .data(|data| data.get_temp::<JsonDraft>(id))
        .filter(|draft| draft.observed == stored)
        .unwrap_or_else(|| JsonDraft {
            observed: stored.clone(),
            text: stored,
        });
    let response = ui.add(
        egui::TextEdit::multiline(&mut draft.text)
            .code_editor()
            .desired_width(f32::INFINITY)
            .desired_rows(3),
    );
    let mut replacement = None;
    match serde_json::from_str::<Value>(&draft.text) {
        Ok(value) if response.changed() => {
            draft.observed = serde_json::to_string_pretty(&value).unwrap();
            replacement = Some(value);
        }
        Err(error) => {
            ui.label(
                RichText::new(format!(
                    "Not applied: {error}. Correct this JSON to update the input."
                ))
                .size(12.0)
                .color(ERROR),
            );
        }
        _ => {}
    }
    ui.data_mut(|data| data.insert_temp(id, draft));
    replacement
}

fn switch(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(44.0, 26.0), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label)
    });
    let amount = ui.ctx().animate_bool(response.id, *value);
    let fill = if *value {
        BLUE
    } else {
        Color32::from_rgb(214, 219, 228)
    };
    ui.painter().rect_filled(rect, 13.0, fill);
    let center = egui::pos2(
        egui::lerp((rect.left() + 13.0)..=(rect.right() - 13.0), amount),
        rect.center().y,
    );
    ui.painter().circle_filled(center, 10.0, Color32::WHITE);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            15.0,
            egui::Stroke::new(1.5_f32, BLUE),
            egui::StrokeKind::Outside,
        );
    }
    response
}

fn card<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(Color32::from_rgb(255, 255, 254))
        .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(226, 231, 239)))
        .corner_radius(16)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            body(ui)
        })
}

fn secondary(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(SECONDARY));
}

fn issue(ui: &mut egui::Ui, title: &str, detail: &str) {
    card(ui, |ui| {
        ui.label(RichText::new(title).strong().color(ERROR));
        ui.label(RichText::new(detail).size(12.0).color(ERROR));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(context: &egui::Context, source: &str, parameters: &mut String) -> Vec<String> {
        let output = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                ui.set_max_width(520.0);
                show(ui, source, parameters, true);
            });
        });
        output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn forms_follow_the_script_and_never_materialise_defaults_on_view() {
        let context = egui::Context::default();
        let mut parameters = "{\"opaque\":{\"items\":[1,2,3]}}".to_owned();
        let before = parameters.clone();
        let first = r#"/* @controls {"flux":{"type":"number","label":"Flux setting","default":0.25},"live":{"type":"boolean","label":"Enable branching","default":true}} */"#;
        let first_text = render(&context, first, &mut parameters).join("\n");
        assert!(first_text.contains("Flux setting"));
        assert!(first_text.contains("Enable branching"));
        assert_eq!(parameters, before);
        let second = r#"/* @controls {"phrase":{"type":"text","label":"Origin phrase","default":"hello"}} */"#;
        let second_text = render(&context, second, &mut parameters).join("\n");
        assert!(second_text.contains("Origin phrase"));
        assert!(!second_text.contains("Flux setting"));
        assert!(!second_text.contains("Enable branching"));
        assert_eq!(parameters, before);
    }

    #[test]
    fn invalid_values_metadata_and_json_remain_untouched_and_visible() {
        let source =
            r#"/* @controls {"flux":{"type":"number","label":"Flux setting","default":0.25}} */"#;
        for (script, stored, expected) in [
            (
                source,
                "{\"flux\":\"keep me\",\"unknown\":42}",
                "expected a number",
            ),
            (source, "{unfinished", "Invalid JSON"),
            (source, "[1,2,3]", "must be a JSON object"),
            (
                "/* @controls broken */",
                "{\"unknown\":42}",
                "Controls could not be read",
            ),
        ] {
            let context = egui::Context::default();
            let mut parameters = stored.to_owned();
            let text = render(&context, script, &mut parameters).join("\n");
            assert!(text.contains(expected), "{text}");
            assert_eq!(parameters, stored);
        }
    }
}
