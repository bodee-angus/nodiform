//! A deliberately small native JavaScript rule editor.
//!
//! This is syntax highlighting and editing assistance, not a language server.
//! A single scroll surface keeps the gutter and source lines together.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, RichText, TextFormat};
use std::ops::Range;

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 21.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScriptStyle {
    Builder,
    Generator,
}

#[derive(Clone, Copy)]
enum Snippet {
    Node,
    Connection,
    Pause,
    Loop,
    Controls,
}

impl Snippet {
    fn text(self, style: ScriptStyle) -> &'static str {
        match (self, style) {
            (Self::Node, ScriptStyle::Builder) =>
                "graph.add('new-node', { color: '#007aff' });\n",
            (Self::Connection, ScriptStyle::Builder) =>
                "graph.connect('source', 'target', { strength: 1, color: '#72859b' });\n",
            (Self::Pause, ScriptStyle::Builder) => "graph.wait(120);\n",
            (Self::Loop, ScriptStyle::Builder) =>
                "for (let i = 1; i <= 10; i++) {\n  graph.add(i);\n  graph.wait(12);\n}\n",
            (Self::Node, ScriptStyle::Generator) =>
                "yield N.batch([N.node('new-node', { color: '#007aff' })], []);\n",
            (Self::Connection, ScriptStyle::Generator) =>
                "yield N.batch([], [N.edge('source', 'target', { id: 'connection', strength: 1 })]);\n",
            (Self::Pause, ScriptStyle::Generator) => "yield N.wait(120);\n",
            (Self::Loop, ScriptStyle::Generator) =>
                "for (let i = 1; i <= 10; i++) {\n  yield N.batch([N.node(String(i))], []);\n  yield N.wait(12);\n}\n",
            (Self::Controls, _) =>
                "/* @controls\n{\n  \"count\": { \"type\": \"integer\", \"label\": \"Count\", \"default\": 10, \"min\": 1, \"max\": 500 }\n}\n*/\n\n",
        }
    }
}

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

    let style = script_style(source);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("JavaScript")
                .size(12.0)
                .color(Color32::from_rgb(99, 104, 115)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("Reference", |ui| {
                reference(ui, style);
            });
            ui.add_enabled_ui(enabled, |ui| {
                ui.menu_button("Insert", |ui| {
                    ui.set_min_width(190.0);
                    for (label, choice, tip) in [
                        ("Node", Snippet::Node, "Add a node with a unique ID."),
                        (
                            "Connection",
                            Snippet::Connection,
                            "Connect two existing nodes.",
                        ),
                        (
                            "Pause",
                            Snippet::Pause,
                            "Let the simulation move for 120 ticks.",
                        ),
                        (
                            "Loop",
                            Snippet::Loop,
                            "Repeat a sequence of graph operations.",
                        ),
                    ] {
                        if ui.button(label).on_hover_text(tip).clicked() {
                            snippet = Some(choice);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            !source.contains("@controls"),
                            egui::Button::new("Optional controls"),
                        )
                        .on_hover_text(
                            "Declare controls for this experiment at the top of the script.",
                        )
                        .clicked()
                    {
                        snippet = Some(Snippet::Controls);
                        ui.close_menu();
                    }
                });
            });
        });
    });
    ui.add_space(7.0);

    let mut inserted = false;
    if let Some(snippet) = snippet {
        let saved = ui.data(|data| data.get_temp::<(usize, usize)>(selection_id));
        let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
        let mut undoer = state.undoer();
        let old_cursor = state.cursor.char_range().unwrap_or_default();
        undoer.add_undo(&(old_cursor, source.clone()));
        let cursor = insert_snippet(source, saved, snippet, style);
        let cursor_range = egui::text::CCursorRange::one(egui::text::CCursor::new(cursor));
        state.cursor.set_char_range(Some(cursor_range));
        undoer.add_undo(&(cursor_range, source.clone()));
        state.set_undoer(undoer);
        state.store(ui.ctx(), editor_id);
        ui.data_mut(|data| data.insert_temp(selection_id, (cursor, cursor)));
        inserted = true;
    }

    let line_count = source.bytes().filter(|b| *b == b'\n').count() + 1;
    let gutter = (1..=line_count)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mut gutter_job = LayoutJob::default();
    gutter_job.append(&gutter, 0.0, format(Color32::from_rgb(159, 165, 176)));
    gutter_job.halign = egui::Align::RIGHT;
    let height = (ui.available_height() - 54.0).max(LINE_HEIGHT);
    let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| {
        ui.fonts(|fonts| fonts.layout_job(highlight(text)))
    };

    let result = egui::Frame::new()
        .fill(Color32::from_rgb(255, 255, 254))
        .inner_margin(14.0)
        .corner_radius(16)
        .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(225, 228, 234)))
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

fn reference(ui: &mut egui::Ui, style: ScriptStyle) {
    ui.set_width(380.0);
    egui::ScrollArea::vertical()
        .max_height(390.0)
        .show(ui, |ui| {
            ui.label(RichText::new("Your experiment, in order").strong());
            ui.add_space(5.0);
            let (intro, entries): (&str, &[(&str, &str)]) = match style {
                ScriptStyle::Builder => (
                    "Define function build(graph, p). Graph operations run in the order you write them; waits set the pace.",
                    &[
                        ("graph.add(id, { color, radius })", "Add a node. A number or string becomes its unique ID."),
                        ("graph.connect(from, to, { strength, color })", "Connect existing nodes. The target can be one ID or an array of IDs."),
                        ("graph.ids() · graph.others(id)", "Get node IDs in creation order, optionally excluding one node."),
                        ("graph.wait(ticks)", "Let the graph move for this many ticks before the next operation."),
                        ("graph.setNode(id, { color, radius })", "Change a node already in the graph."),
                        ("graph.setEdge(edgeId, { strength, color })", "Change a connection. connect() returns an array of edge IDs."),
                        ("graph.random()", "Get a repeatable random number from this run's seed."),
                    ],
                ),
                ScriptStyle::Generator => (
                    "Define function* generate(N, params). Each yield produces an ordered event. Existing generator scripts remain supported.",
                    &[
                        ("N.node(id, { color, radius })", "Describe a node with a unique string ID."),
                        ("N.edge(from, to, { id, strength, color })", "Describe a connection with a unique ID."),
                        ("yield N.batch(nodes, edges)", "Introduce nodes and connections together."),
                        ("yield N.wait(ticks)", "Let the graph move for this many ticks before the next event."),
                        ("yield N.setNode(id, { color, radius })", "Change an existing node."),
                        ("yield N.setEdge(id, { strength, color })", "Change an existing connection."),
                        ("N.random()", "Get a repeatable random number from this run's seed."),
                    ],
                ),
            };
            ui.label(RichText::new(intro).size(12.0));
            for (signature, meaning) in entries {
                ui.add_space(9.0);
                ui.label(RichText::new(*signature).monospace().size(11.0).color(Color32::from_rgb(0, 103, 183)));
                ui.label(RichText::new(*meaning).size(12.0));
            }
            ui.add_space(12.0);
            ui.separator();
            ui.label(RichText::new("Optional controls").strong());
            ui.label(RichText::new("A leading @controls JSON comment declares this experiment's fields. Use Insert → Optional controls for a starting point. Read values from p in build(), or params in generate(). Scripts without controls need no parameter fields.").size(12.0));
            ui.add_space(8.0);
            ui.label(RichText::new("Insert uses your last text selection and matches this script's API. With no selection, it adds code at the end of the experiment function. JavaScript syntax is highlighted; Validate checks the script.").size(11.0).weak());
        });
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
            Kind::Plain => Color32::from_rgb(39, 44, 55),
            Kind::Keyword => Color32::from_rgb(135, 54, 165),
            Kind::String => Color32::from_rgb(43, 116, 66),
            Kind::Comment => Color32::from_rgb(119, 127, 139),
            Kind::Number => Color32::from_rgb(174, 91, 28),
            Kind::Api => Color32::from_rgb(0, 103, 183),
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
                    "N" | "graph" | "add" | "connect" | "ids" | "others" | "node" | "edge"
                    | "batch" | "wait" | "setNode" | "setEdge" | "random" => Kind::Api,
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

fn script_style(source: &str) -> ScriptStyle {
    if function_body(source, "build").is_some() {
        ScriptStyle::Builder
    } else if function_body(source, "generate").is_some() {
        ScriptStyle::Generator
    } else {
        ScriptStyle::Builder
    }
}

/// Locate the declared experiment body, keeping nested blocks together and
/// ignoring braces in strings and comments. This is editing assistance, not a
/// JavaScript parser; validation belongs to the rule engine.
fn function_body(source: &str, name: &str) -> Option<Range<usize>> {
    let spans: Vec<_> = tokens(source)
        .into_iter()
        .filter(|(range, kind)| *kind != Kind::Comment && !source[range.clone()].trim().is_empty())
        .collect();
    for (index, (range, kind)) in spans.iter().enumerate() {
        if *kind != Kind::Keyword || &source[range.clone()] != "function" {
            continue;
        }
        let mut next = index + 1;
        if spans
            .get(next)
            .is_some_and(|(range, _)| &source[range.clone()] == "*")
        {
            next += 1;
        }
        let Some((function_name, name_kind)) = spans.get(next) else {
            continue;
        };
        if *name_kind == Kind::String || &source[function_name.clone()] != name {
            continue;
        }
        next += 1;
        if !spans
            .get(next)
            .is_some_and(|(range, _)| &source[range.clone()] == "(")
        {
            continue;
        }
        let mut parentheses = 0;
        for (range, kind) in &spans[next..] {
            next += 1;
            if *kind != Kind::Plain {
                continue;
            }
            match &source[range.clone()] {
                "(" => parentheses += 1,
                ")" => {
                    parentheses -= 1;
                    if parentheses == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some((open, _)) = spans.get(next) else {
            continue;
        };
        if &source[open.clone()] != "{" {
            continue;
        }
        let mut depth = 0;
        for (range, kind) in &spans[next..] {
            if *kind != Kind::Plain {
                continue;
            }
            match &source[range.clone()] {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open.start..range.end);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn insertion_range(source: &str, saved: Option<(usize, usize)>) -> (Range<usize>, usize) {
    if let Some((a, b)) = saved {
        let start = char_to_byte(source, a.min(b));
        let end = char_to_byte(source, a.max(b));
        return (start..end, source[..start].chars().count());
    }
    let name = match script_style(source) {
        ScriptStyle::Builder => "build",
        ScriptStyle::Generator => "generate",
    };
    let byte = function_body(source, name)
        .map(|range| range.end - 1)
        .unwrap_or(source.len());
    (byte..byte, source[..byte].chars().count())
}

fn insert_snippet(
    source: &mut String,
    saved: Option<(usize, usize)>,
    snippet: Snippet,
    style: ScriptStyle,
) -> usize {
    let text = snippet.text(style);
    if matches!(snippet, Snippet::Controls) {
        source.insert_str(0, text);
        return text.chars().count();
    }
    if source.trim().is_empty() {
        let body = text.trim_end_matches('\n').replace('\n', "\n  ");
        *source = format!("function build(graph, p) {{\n  {body}\n}}\n");
        return source[..source.rfind('}').unwrap()].chars().count();
    }
    let (range, cursor) = insertion_range(source, saved);
    let line_start = source[..range.start].rfind('\n').map_or(0, |p| p + 1);
    let before = &source[line_start..range.start];
    let base_indent: String = before
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .collect();
    let at_closing_brace = source[range.end..].starts_with('}');
    let indent = if at_closing_brace {
        format!("{base_indent}{}", indentation_unit(source))
    } else {
        base_indent.clone()
    };
    let mut replacement = if before.trim().is_empty() {
        indent.strip_prefix(before).unwrap_or(&indent).to_owned()
    } else {
        format!("\n{indent}")
    };
    for (index, line) in text.trim_end_matches('\n').lines().enumerate() {
        if index > 0 {
            replacement.push('\n');
            replacement.push_str(&indent);
        }
        let leading = line.len() - line.trim_start_matches(' ').len();
        replacement.push_str(&indentation_unit(source).repeat(leading / 2));
        replacement.push_str(&line[leading..]);
    }
    replacement.push('\n');
    replacement.push_str(&base_indent);
    let cursor = cursor + replacement.chars().count();
    source.replace_range(range, &replacement);
    cursor
}

fn indentation_unit(source: &str) -> &str {
    for line in source.lines() {
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        if indent > 0 && !line.trim().is_empty() {
            if line.starts_with('\t') {
                return "\t";
            }
            return &line[..indent];
        }
    }
    "  "
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
    fn builder_insertion_uses_experiment_body_not_trailing_helper() {
        let mut source = "function build(graph, p) {\n  for (const item of ['}']) {\n    graph.add(item); // }\n  }\n}\nfunction helper() { return { value: 1 }; }".to_owned();
        assert_eq!(script_style(&source), ScriptStyle::Builder);
        insert_snippet(&mut source, None, Snippet::Pause, ScriptStyle::Builder);
        assert!(source.contains("  }\n  graph.wait(120);\n}\nfunction helper()"));
    }

    #[test]
    fn multiline_snippet_preserves_indentation_and_cursor() {
        let mut source = "function build(graph, p) {\n    graph.add('first');\n}".to_owned();
        let cursor = insert_snippet(&mut source, None, Snippet::Loop, ScriptStyle::Builder);
        assert!(source.contains("    for (let i = 1; i <= 10; i++) {\n        graph.add(i);\n        graph.wait(12);\n    }\n}"));
        assert_eq!(&source[char_to_byte(&source, cursor)..], "}");
    }

    #[test]
    fn selected_unicode_statement_is_replaced_without_losing_indent() {
        let mut source = "function build(graph, p) {\n  graph.add('λ');\n}".to_owned();
        let start = source[..source.find("graph.add").unwrap()].chars().count();
        let end = start + "graph.add('λ');".chars().count();
        insert_snippet(
            &mut source,
            Some((end, start)),
            Snippet::Pause,
            ScriptStyle::Builder,
        );
        assert!(!source.contains('λ'));
        assert!(source.contains("\n  graph.wait(120);\n"));
    }

    #[test]
    fn strings_and_comments_do_not_select_the_builder_api() {
        let source = "// function build(graph) {}\nconst hint = 'function build(graph) {}';\nfunction* generate(N, params) { yield N.wait(1); }";
        assert_eq!(script_style(source), ScriptStyle::Generator);
        let mut source = source.to_owned();
        insert_snippet(&mut source, None, Snippet::Pause, ScriptStyle::Generator);
        assert!(source.contains("yield N.wait(120);"));
        assert!(!source.contains("graph.wait"));
    }

    #[test]
    fn controls_are_inserted_before_the_function_and_ignore_selection() {
        let original = "function build(graph, p) {}";
        let mut source = original.to_owned();
        let cursor = insert_snippet(
            &mut source,
            Some((5, 15)),
            Snippet::Controls,
            ScriptStyle::Builder,
        );
        assert!(source.starts_with("/* @controls\n"));
        assert_eq!(&source[char_to_byte(&source, cursor)..], original);
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
