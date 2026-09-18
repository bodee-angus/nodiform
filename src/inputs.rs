//! A source-driven form. The application has no knowledge of experiment-specific keys.
use crate::experiment::{
    is_safe_integer, parse_controls, validate_parameters, validate_value, ControlKind, ControlSpec,
    MAX_SAFE_INTEGER,
};
use crate::theme::Palette;
use eframe::egui::{self, Color32, RichText};
use serde_json::Value;

/// Optional controls are declared in the script. Merely viewing a default never
/// changes the stored JSON; changing a field preserves every other parameter.
pub fn show(ui: &mut egui::Ui, source: &str, parameters_text: &mut String, enabled: bool) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
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
                        let stored_input_count = values.as_object().unwrap().len();
                        if controls.is_empty() {
                            card(ui, |ui| {
                                if stored_input_count == 0 {
                                    ui.label(
                                        RichText::new("Everything starts in the rules").strong(),
                                    );
                                    secondary(ui, "This script does not declare any inputs. Define values in the editor, or use Insert → Optional controls to expose the values you want to adjust here.");
                                } else {
                                    ui.label(RichText::new("Stored inputs are available").strong());
                                    secondary(
                                        ui,
                                        &format!(
                                            "{stored_input_count} stored {} from this metadata-free script can be edited in Advanced Input JSON below.",
                                            if stored_input_count == 1 { "input" } else { "inputs" }
                                        ),
                                    );
                                }
                            });
                        }
                        reveal_json = (controls.is_empty() && stored_input_count > 0)
                            || validate_parameters(controls, &values).is_err();
                        let mut changed = false;
                        // Source is part of the ID so switching scripts never reuses an
                        // unfinished text or JSON draft from a different experiment.
                        ui.push_id(source, |ui| {
                            ui.add_enabled_ui(enabled, |ui| {
                                for (name, spec) in controls {
                                    let explicit = values.get(name).is_some();
                                    let current = values.get(name).unwrap_or(&spec.default);
                                    let action = ui.push_id(name, |ui| {
                                        control_card(ui, name, spec, current, explicit)
                                    }).inner;
                                    if let Some(action) = action {
                                        apply_action(&mut values, name, action);
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
) -> Option<ControlAction> {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&spec.label).size(14.0).strong());
            if !explicit {
                ui.label(RichText::new("Default").size(11.0).color(Palette::for_ui(ui).secondary));
            }
        });
        if let Some(description) = &spec.description {
            secondary(ui, description);
        }
        let validation = validate_value(spec, current);
        let mut action = if compatible_type(spec.kind, current) {
            field(ui, spec, current, explicit)
        } else {
            secondary(ui, "The saved value has a different type. Edit Input JSON below, or explicitly use the script's default.");
            ui.button("Use default").clicked().then_some(ControlAction::Clear)
        };
        if let Err(error) = validation {
            ui.label(RichText::new(format!("{name}: {error}")).size(12.0).color(Palette::for_ui(ui).error));
        }
        if explicit {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("params.{name}")).monospace().size(11.0).color(Palette::for_ui(ui).secondary));
                if ui.small_button("Reset value").on_hover_text("Use the default declared in this script.").clicked() {
                    action = Some(ControlAction::Clear);
                }
            });
        }
        action
    }).inner
}

#[derive(Clone, Debug, PartialEq)]
enum ControlAction {
    Set(Value),
    Clear,
}

fn apply_action(values: &mut Value, name: &str, action: ControlAction) {
    let values = values
        .as_object_mut()
        .expect("input actions are applied only to parameter objects");
    match action {
        ControlAction::Set(value) => {
            values.insert(name.to_owned(), value);
        }
        ControlAction::Clear => {
            values.remove(name);
        }
    }
}

fn compatible_type(kind: ControlKind, value: &Value) -> bool {
    match kind {
        ControlKind::Integer | ControlKind::Number => value.is_number(),
        ControlKind::Boolean => value.is_boolean(),
        ControlKind::Text | ControlKind::Color | ControlKind::Select => value.is_string(),
        ControlKind::Json => true,
    }
}

fn field(
    ui: &mut egui::Ui,
    spec: &ControlSpec,
    current: &Value,
    explicit: bool,
) -> Option<ControlAction> {
    let replacement = match spec.kind {
        ControlKind::Integer | ControlKind::Number => {
            let mut number = current.as_f64().unwrap();
            let step = spec.step.unwrap_or(if spec.kind == ControlKind::Integer {
                1.0
            } else {
                0.1
            });
            let mut widget = egui::DragValue::new(&mut number)
                .speed(step)
                .range(numeric_range(spec))
                // Validate typed values before DragValue clamps them. An unsafe
                // integer must never silently become a different valid count.
                .custom_parser(|text| parse_numeric_input(spec, text))
                .custom_formatter(|number, _| number.to_string())
                .clamp_existing_to_range(false);
            if spec.kind == ControlKind::Integer {
                widget = widget.max_decimals(0);
            }
            if ui.add(widget).changed() {
                numeric_value(spec, number)
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
        ControlKind::Text => string_field(ui, spec, current, explicit, false),
        ControlKind::Color => string_field(ui, spec, current, explicit, true),
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
        ControlKind::Json => json_field(ui, current, explicit),
    };
    replacement
        .filter(|value| validate_value(spec, value).is_ok())
        .map(ControlAction::Set)
}

fn numeric_range(spec: &ControlSpec) -> std::ops::RangeInclusive<f64> {
    let limit = if spec.kind == ControlKind::Integer {
        MAX_SAFE_INTEGER
    } else {
        f64::MAX
    };
    spec.min.unwrap_or(-limit)..=spec.max.unwrap_or(limit)
}

fn numeric_value(spec: &ControlSpec, number: f64) -> Option<Value> {
    let value = if spec.kind == ControlKind::Integer {
        if !is_safe_integer(number) {
            return None;
        }
        Value::from(number as i64)
    } else {
        Value::Number(serde_json::Number::from_f64(number)?)
    };
    validate_value(spec, &value).ok()?;
    Some(value)
}

fn parse_numeric_input(spec: &ControlSpec, text: &str) -> Option<f64> {
    let number = text.trim().parse::<f64>().ok()?;
    numeric_value(spec, number)?;
    Some(number)
}

#[derive(Clone)]
struct FieldDraft {
    observed: String,
    observed_explicit: bool,
    text: String,
}

impl FieldDraft {
    fn observes(&self, value: &str, explicit: bool) -> bool {
        self.observed == value && self.observed_explicit == explicit
    }
}

fn string_field(
    ui: &mut egui::Ui,
    spec: &ControlSpec,
    current: &Value,
    explicit: bool,
    color: bool,
) -> Option<Value> {
    let id = ui.make_persistent_id(if color { "color-draft" } else { "text-draft" });
    let stored = current.as_str().unwrap();
    let mut draft = ui
        .data(|data| data.get_temp::<FieldDraft>(id))
        .filter(|draft| draft.observes(stored, explicit))
        .unwrap_or_else(|| FieldDraft {
            observed: stored.to_owned(),
            observed_explicit: explicit,
            text: stored.to_owned(),
        });
    let changed = if color {
        let mut changed = false;
        ui.horizontal(|ui| {
            if let Ok(value) = crate::model::parse_color(&draft.text)
                .or_else(|_| crate::model::parse_color(stored))
            {
                let mut channels = value.map(|channel| (channel * 255.0).round() as u8);
                if ui
                    .color_edit_button_srgba_unmultiplied(&mut channels)
                    .changed()
                {
                    draft.text = format!(
                        "#{:02x}{:02x}{:02x}{:02x}",
                        channels[0], channels[1], channels[2], channels[3]
                    );
                    changed = true;
                }
            }
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut draft.text)
                        .hint_text("#RRGGBB or #RRGGBBAA")
                        .desired_width(ui.available_width()),
                )
                .changed();
        });
        changed
    } else {
        ui.add(egui::TextEdit::singleline(&mut draft.text).desired_width(f32::INFINITY))
            .changed()
    };
    let edited = draft.text != stored;
    let replacement = match validate_string_draft(spec, &mut draft, changed) {
        Ok(value) => value,
        Err(error) => {
            if edited {
                ui.label(
                    RichText::new(format!(
                        "Not applied: {error}. Correct this value to update the input."
                    ))
                    .size(12.0)
                    .color(Palette::for_ui(ui).error),
                );
            }
            None
        }
    };
    ui.data_mut(|data| data.insert_temp(id, draft));
    replacement
}

fn validate_string_draft(
    spec: &ControlSpec,
    draft: &mut FieldDraft,
    changed: bool,
) -> Result<Option<Value>, String> {
    let value = Value::String(draft.text.clone());
    validate_value(spec, &value)?;
    if changed {
        draft.observed.clone_from(&draft.text);
        draft.observed_explicit = true;
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

fn json_field(ui: &mut egui::Ui, current: &Value, explicit: bool) -> Option<Value> {
    let id = ui.make_persistent_id("json-draft");
    let stored = serde_json::to_string_pretty(current).unwrap();
    let mut draft = ui
        .data(|data| data.get_temp::<FieldDraft>(id))
        .filter(|draft| draft.observes(&stored, explicit))
        .unwrap_or_else(|| FieldDraft {
            observed: stored.clone(),
            observed_explicit: explicit,
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
            draft.observed_explicit = true;
            replacement = Some(value);
        }
        Err(error) => {
            ui.label(
                RichText::new(format!(
                    "Not applied: {error}. Correct this JSON to update the input."
                ))
                .size(12.0)
                .color(Palette::for_ui(ui).error),
            );
        }
        _ => {}
    }
    ui.data_mut(|data| data.insert_temp(id, draft));
    replacement
}

pub(crate) fn switch(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
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
        Palette::for_ui(ui).action
    } else {
        Palette::for_ui(ui).switch_off
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
            egui::Stroke::new(1.5_f32, Palette::for_ui(ui).action),
            egui::StrokeKind::Outside,
        );
    }
    response
}

fn card<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(Palette::for_ui(ui).card)
        .stroke(egui::Stroke::new(1.0_f32, Palette::for_ui(ui).line))
        .corner_radius(16)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            body(ui)
        })
}

fn secondary(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(12.0)
            .color(Palette::for_ui(ui).secondary),
    );
}

fn issue(ui: &mut egui::Ui, title: &str, detail: &str) {
    card(ui, |ui| {
        ui.label(
            RichText::new(title)
                .strong()
                .color(Palette::for_ui(ui).error),
        );
        ui.label(
            RichText::new(detail)
                .size(12.0)
                .color(Palette::for_ui(ui).error),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn metadata_free_scripts_reveal_existing_custom_inputs() {
        let context = egui::Context::default();
        let source = "function build(graph, p) { graph.add(p.firstNode); }";
        let mut parameters = r#"{"firstNode":"legacy","weights":[1,2]}"#.to_owned();
        let before = parameters.clone();
        let text = render(&context, source, &mut parameters).join("\n");
        assert!(text.contains("Stored inputs are available"), "{text}");
        assert!(text.contains("2 stored inputs"), "{text}");
        assert!(text.contains("firstNode"), "{text}");
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

    #[test]
    fn clear_action_removes_override_so_source_default_can_change() {
        let mut values = json!({"count": 9, "opaque": {"keep": true}});
        apply_action(&mut values, "count", ControlAction::Clear);
        assert_eq!(values, json!({"opaque": {"keep": true}}));

        let first = crate::experiment::parse_controls(
            r#"/* @controls {"count":{"type":"integer","label":"Count","default":3}} */"#,
        )
        .unwrap();
        let second = crate::experiment::parse_controls(
            r#"/* @controls {"count":{"type":"integer","label":"Count","default":12}} */"#,
        )
        .unwrap();
        assert_eq!(
            crate::experiment::merge_defaults(&first, &values).unwrap()["count"],
            3
        );
        assert_eq!(
            crate::experiment::merge_defaults(&second, &values).unwrap()["count"],
            12
        );

        apply_action(&mut values, "count", ControlAction::Set(json!(7)));
        assert_eq!(values["count"], 7);
        assert_eq!(values["opaque"], json!({"keep": true}));
    }

    #[test]
    fn invalid_string_drafts_stay_visible_without_becoming_parameters() {
        let controls = crate::experiment::parse_controls(
            r##"/* @controls {
                "tint":{"type":"color","label":"Tint","default":"#112233"},
                "title":{"type":"text","label":"Title","default":"short"}
            } */"##,
        )
        .unwrap();

        let mut color = FieldDraft {
            observed: "#112233".into(),
            observed_explicit: true,
            text: "#f".into(),
        };
        let mut values = json!({"tint":"#112233"});
        let invalid = validate_string_draft(&controls["tint"], &mut color, true);
        assert!(invalid.is_err());
        if let Ok(Some(value)) = invalid {
            apply_action(&mut values, "tint", ControlAction::Set(value));
        }
        assert_eq!(values["tint"], "#112233");
        assert_eq!(color.observed, "#112233");
        assert_eq!(color.text, "#f");
        assert!(color.observes("#112233", true));
        assert!(!color.observes("#112233", false));

        color.text = "#abcdef".into();
        let corrected = validate_string_draft(&controls["tint"], &mut color, true)
            .unwrap()
            .unwrap();
        apply_action(&mut values, "tint", ControlAction::Set(corrected));
        assert_eq!(values["tint"], "#abcdef");
        assert_eq!(color.observed, "#abcdef");
        assert!(color.observed_explicit);

        let mut text = FieldDraft {
            observed: "short".into(),
            observed_explicit: true,
            text: "x".repeat(16_385),
        };
        assert!(validate_string_draft(&controls["title"], &mut text, true).is_err());
        assert_eq!(text.observed, "short");
        assert_eq!(text.text.len(), 16_385);
    }

    #[test]
    fn large_numeric_inputs_roundtrip_without_a_hidden_million_limit() {
        let source = r#"/* @controls {
            "count":{"type":"integer","label":"Count","default":2000000},
            "scale":{"type":"number","label":"Scale","default":1e100},
            "bounded":{"type":"integer","label":"Bounded","default":1,"min":1,"max":2000000}
        } */"#;
        let controls = parse_controls(source).unwrap();
        let count = &controls["count"];
        assert_eq!(numeric_range(count), -MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER);
        for integer in [2_000_000_i64, 9_000_000_000, 9_007_199_254_740_991] {
            let parsed = parse_numeric_input(count, &integer.to_string()).unwrap();
            assert_eq!(numeric_value(count, parsed), Some(json!(integer)));
        }
        for invalid in [
            "9007199254740992",
            "9007199254740993",
            "2000000.5",
            "NaN",
            "inf",
        ] {
            assert!(parse_numeric_input(count, invalid).is_none(), "{invalid}");
        }
        assert_eq!(parse_numeric_input(count, "2e6"), Some(2_000_000.0));
        assert!(parse_numeric_input(&controls["bounded"], "2000001").is_none());
        assert_eq!(numeric_range(&controls["scale"]), -f64::MAX..=f64::MAX);
        assert_eq!(
            parse_numeric_input(&controls["scale"], "1e100"),
            Some(1e100)
        );

        let context = egui::Context::default();
        let mut parameters = r#"{"count":9007199254740991}"#.to_owned();
        let before = parameters.clone();
        let text = render(&context, source, &mut parameters).join("\n");
        assert!(text.contains("9007199254740991"), "{text}");
        assert!(!text.contains("within ±1000000"), "{text}");
        assert_eq!(parameters, before);
    }
}
