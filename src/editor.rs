//! A deliberately small native JavaScript rule editor.
//!
//! This is syntax highlighting and editing assistance, not a language server.
//! A single scroll surface keeps the gutter and source lines together.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, RichText, TextFormat};
use std::ops::Range;

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 21.0;

const NODE_SNIPPET: &str = "yield N.batch([N.node('new-node', { color: '#79dfc7' })], []);\n";
const EDGE_SNIPPET: &str =
    "yield N.batch([], [N.edge('A', 'B', { id: 'A-B', strength: 1, color: '#668d9c' })]);\n";
const WAIT_SNIPPET: &str = "yield N.wait(120);\n";

/// Draw the editor in the available area. The response is marked changed when
/// either typing or a snippet button modifies the source.
pub fn show(ui: &mut egui::Ui, source: &mut String, enabled: bool) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
        ui.spacing_mut().button_padding = egui::vec2(9.0, 4.0);
        show_inner(ui, source, enabled)
    })
    .inner
}

fn show_inner(ui: &mut egui::Ui, source: &mut String, enabled: bool) -> egui::Response {
    let editor_id = ui.make_persistent_id("nodiform-rule-editor");
    let selection_id = editor_id.with("selection");
    let mut snippet = None;

    ui.horizontal(|ui| {
        ui.label(RichText::new("RULE EDITOR").size(10.0).strong().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new("JavaScript").size(11.0).weak());
        });
    });
    ui.add_space(4.0);
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Insert").size(11.0).weak());
            for (label, text, tip) in [
                (
                    "Node",
                    NODE_SNIPPET,
                    "Insert a new node at the cursor. Give it a unique ID.",
                ),
                (
                    "Edge",
                    EDGE_SNIPPET,
                    "Insert a connection. Replace A and B with existing node IDs.",
                ),
                (
                    "Pause",
                    WAIT_SNIPPET,
                    "Let the graph move for 120 simulation ticks before continuing.",
                ),
            ] {
                if ui.small_button(label).on_hover_text(tip).clicked() {
                    snippet = Some(text);
                }
            }
        });
    });

    egui::CollapsingHeader::new(RichText::new("Rule reference").size(11.0))
        .id_salt(editor_id.with("reference"))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(editor_id.with("reference-scroll"))
                .max_height((ui.available_height() * 0.3).clamp(60.0, 150.0))
                .show(ui, |ui| {
            ui.label("Define function* generate(N, params). Each yield creates one ordered event.");
            for (signature, meaning) in [
                ("N.node(id, { color })", "Describe a node with a stable, unique ID."),
                ("N.edge(from, to, { id, strength, color })", "Describe a weighted connection with a unique ID."),
                ("yield N.batch(nodes, edges)", "Introduce nodes and connections together."),
                ("yield N.wait(ticks)", "Allow physical movement before the next event."),
                ("yield N.setNode(id, { color })", "Update an existing node."),
                ("yield N.setEdge(id, { strength })", "Update an existing edge."),
                ("N.random()", "Use the run's seeded random number source."),
            ] {
                ui.add_space(3.0);
                ui.label(RichText::new(signature).monospace().size(11.0));
                ui.label(RichText::new(meaning).size(11.0).weak());
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Insert buttons use the last cursor position. Before editing, they insert before the generator's closing brace. This is JavaScript, not TypeScript.").size(11.0).weak());
                });
        });
    ui.add_space(5.0);

    let mut inserted = false;
    if let Some(snippet) = snippet {
        let saved = ui.data(|data| data.get_temp::<(usize, usize)>(selection_id));
        let (range, cursor) = insertion_range(source, saved);
        source.replace_range(range, snippet);
        let cursor = cursor + snippet.chars().count();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), editor_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(cursor),
                )));
            state.store(ui.ctx(), editor_id);
        }
        ui.data_mut(|data| data.insert_temp(selection_id, (cursor, cursor)));
        inserted = true;
    }

    let line_count = source.bytes().filter(|b| *b == b'\n').count() + 1;
    let gutter = (1..=line_count)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mut gutter_job = LayoutJob::default();
    gutter_job.append(&gutter, 0.0, format(Color32::from_rgb(89, 112, 125)));
    gutter_job.halign = egui::Align::RIGHT;
    let height = (ui.available_height() - 49.0).max(LINE_HEIGHT);
    let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| {
        ui.fonts(|fonts| fonts.layout_job(highlight(text)))
    };

    let result = egui::Frame::new()
        .fill(Color32::from_rgb(12, 22, 29))
        .inner_margin(10.0)
        .corner_radius(8)
        .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(38, 57, 68)))
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt(editor_id.with("scroll"))
                .auto_shrink([false, false])
                .max_height(height)
                .show(ui, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 13.0;
                            ui.add(
                                egui::Label::new(gutter_job)
                                    .selectable(false)
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                            egui::TextEdit::multiline(source)
                                .id(editor_id)
                                .code_editor()
                                .font(FontId::monospace(FONT_SIZE))
                                .lock_focus(true)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20)
                                .margin(egui::Vec2::ZERO)
                                .frame(false)
                                .layouter(&mut layouter)
                                .show(ui)
                        })
                        .inner
                    })
                    .inner
                })
                .inner
        })
        .inner;

    if (result.response.has_focus() || result.response.lost_focus()) && !inserted {
        if let Some(range) = result.cursor_range {
            ui.data_mut(|data| {
                data.insert_temp(
                    selection_id,
                    (range.primary.ccursor.index, range.secondary.ccursor.index),
                )
            });
        }
    }
    let mut response = result.response;
    if inserted {
        response.request_focus();
        response.mark_changed();
    }
    ui.horizontal(|ui| {
        let position = result
            .cursor_range
            .map(|r| r.primary.ccursor.index)
            .unwrap_or(0);
        let byte = char_to_byte(source, position);
        let before = &source[..byte];
        let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        ui.label(
            RichText::new(format!("Ln {line}, Col {column}"))
                .size(10.0)
                .weak(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = if enabled {
                "Tab indents · Ctrl+Z undoes"
            } else {
                "Editing locked during this run"
            };
            ui.label(RichText::new(hint).size(10.0).weak());
        });
    });
    response
}

fn format(color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::monospace(FONT_SIZE),
        color,
        line_height: Some(LINE_HEIGHT),
        ..Default::default()
    }
}

fn highlight(source: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (range, kind) in tokens(source) {
        let color = match kind {
            Kind::Plain => Color32::from_rgb(216, 226, 223),
            Kind::Keyword => Color32::from_rgb(181, 164, 232),
            Kind::String => Color32::from_rgb(167, 208, 150),
            Kind::Comment => Color32::from_rgb(115, 145, 156),
            Kind::Number => Color32::from_rgb(228, 177, 137),
            Kind::Api => Color32::from_rgb(121, 223, 199),
        };
        job.append(&source[range], 0.0, format(color));
    }
    job
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Api,
}

/// Small UTF-8-safe lexer; block comments and multiline template strings retain
/// their state across lines. Template interpolations intentionally share the
/// string colour. Highlighting never changes or evaluates the source.
fn tokens(source: &str) -> Vec<(Range<usize>, Kind)> {
    let mut spans = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let kind;
        if source[i..].starts_with("//") {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            kind = Kind::Comment;
        } else if source[i..].starts_with("/*") {
            i += 2;
            if let Some(end) = source[i..].find("*/") {
                i += end + 2;
            } else {
                i = bytes.len();
            }
            kind = Kind::Comment;
        } else if matches!(bytes[i], b'\'' | b'"' | b'`') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 1;
                    if i < bytes.len() {
                        i += source[i..].chars().next().unwrap().len_utf8();
                    }
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += source[i..].chars().next().unwrap().len_utf8();
                }
            }
            kind = Kind::String;
        } else if bytes[i].is_ascii_digit() {
            i = number_end(bytes, i);
            kind = Kind::Number;
        } else {
            let c = source[i..].chars().next().unwrap();
            if c.is_alphabetic() || c == '_' || c == '$' {
                i += c.len_utf8();
                while i < bytes.len() {
                    let next = source[i..].chars().next().unwrap();
                    if !(next.is_alphanumeric() || next == '_' || next == '$') {
                        break;
                    }
                    i += next.len_utf8();
                }
                kind = match &source[start..i] {
                    "function" | "yield" | "const" | "let" | "var" | "for" | "of" | "in"
                    | "while" | "do" | "if" | "else" | "return" | "break" | "continue"
                    | "switch" | "case" | "default" | "new" | "class" | "extends" | "true"
                    | "false" | "null" | "undefined" | "throw" | "try" | "catch" | "finally"
                    | "typeof" | "instanceof" | "delete" | "void" => Kind::Keyword,
                    "N" | "node" | "edge" | "batch" | "wait" | "setNode" | "setEdge" | "random" => {
                        Kind::Api
                    }
                    _ => Kind::Plain,
                };
            } else {
                i += c.len_utf8();
                kind = Kind::Plain;
            }
        }
        spans.push((start..i, kind));
    }
    spans
}

fn number_end(bytes: &[u8], mut i: usize) -> usize {
    if bytes[i] == b'0' && i + 1 < bytes.len() {
        let radix = match bytes[i + 1] {
            b'x' | b'X' => 16,
            b'b' | b'B' => 2,
            b'o' | b'O' => 8,
            _ => 0,
        };
        if radix != 0 {
            i += 2;
            while i < bytes.len() && (bytes[i] == b'_' || (bytes[i] as char).is_digit(radix)) {
                i += 1;
            }
            return i;
        }
    }
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
        i += 1;
        if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
            i += 1;
        }
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
    }
    if i < bytes.len() && bytes[i] == b'n' {
        i += 1;
    }
    i
}

fn char_to_byte(source: &str, index: usize) -> usize {
    source
        .char_indices()
        .nth(index)
        .map(|(i, _)| i)
        .unwrap_or(source.len())
}

fn insertion_range(source: &str, saved: Option<(usize, usize)>) -> (Range<usize>, usize) {
    if let Some((a, b)) = saved {
        let start = char_to_byte(source, a.min(b));
        let end = char_to_byte(source, a.max(b));
        return (start..end, source[..start].chars().count());
    }
    // Ignore braces inside strings and comments when finding the final body.
    let byte = tokens(source)
        .into_iter()
        .rev()
        .find(|(range, kind)| *kind == Kind::Plain && &source[range.clone()] == "}")
        .map(|(range, _)| range.start)
        .unwrap_or(source.len());
    (byte..byte, source[..byte].chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexer_covers_utf8_source_without_changes() {
        let source = "const café = 'λ'; // α\nyield N.wait(120);";
        let parts = tokens(source);
        let joined = parts
            .iter()
            .map(|(r, _)| &source[r.clone()])
            .collect::<String>();
        assert_eq!(joined, source);
        assert!(parts
            .iter()
            .any(|(r, k)| *k == Kind::String && &source[r.clone()] == "'λ'"));
    }

    #[test]
    fn comments_and_strings_span_lines() {
        let source = "/* one\ntwo */ `hello\nworld` '\\'quoted'";
        let parts = tokens(source);
        assert_eq!(parts[0].1, Kind::Comment);
        assert_eq!(&source[parts[0].0.clone()], "/* one\ntwo */");
        assert_eq!(parts.iter().filter(|(_, k)| *k == Kind::String).count(), 2);
    }

    #[test]
    fn numbers_do_not_swallow_operators() {
        let source = "1+2-3 1e-4 0xff";
        let numbers = tokens(source)
            .into_iter()
            .filter(|(_, k)| *k == Kind::Number)
            .map(|(r, _)| &source[r])
            .collect::<Vec<_>>();
        assert_eq!(numbers, ["1", "2", "3", "1e-4", "0xff"]);
    }

    #[test]
    fn default_snippet_location_ignores_comment_braces() {
        let source = "function* generate() {\n}\n// }";
        let (range, _) = insertion_range(source, None);
        assert_eq!(range.start, source.find("}\n").unwrap());
    }

    #[test]
    fn selection_is_in_characters_not_bytes() {
        let (range, cursor) = insertion_range("aλbc", Some((3, 1)));
        assert_eq!(range, 1..4);
        assert_eq!(cursor, 1);
        let (range, _) = insertion_range("hi", Some((999, 999)));
        assert_eq!(range, 2..2);
    }

    #[test]
    fn editor_lays_out_in_a_native_panel_without_mutating_source() {
        let context = egui::Context::default();
        let original = "function* generate(N, params) {\n  yield N.wait(120);\n}";
        let mut source = original.to_owned();
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::SidePanel::left("editor-test")
                    .default_width(500.0)
                    .show(ctx, |ui| {
                        let response = show(ui, &mut source, true);
                        assert!(response.rect.is_finite());
                    });
            },
        );
        assert!(!output.shapes.is_empty());
        assert_eq!(source, original);
    }
}
