//! Optional controls belong to an experiment's source, never to a particular graph family.
use serde::{de::Visitor, Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type Controls = BTreeMap<String, ControlSpec>;
const MAX_CONTROLS: usize = 64;
const MAX_METADATA_BYTES: usize = 65_536;

struct UniqueControls(Controls);

impl<'de> Deserialize<'de> for UniqueControls {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ControlsVisitor;
        impl<'de> Visitor<'de> for ControlsVisitor {
            type Value = UniqueControls;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an object with unique control names")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut values: A,
            ) -> Result<Self::Value, A::Error> {
                let mut controls = Controls::new();
                while let Some((name, spec)) = values.next_entry::<String, ControlSpec>()? {
                    if controls.insert(name.clone(), spec).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate control '{name}'"
                        )));
                    }
                }
                Ok(UniqueControls(controls))
            }
        }
        deserializer.deserialize_map(ControlsVisitor)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ControlKind {
    Integer,
    Number,
    Boolean,
    Text,
    Color,
    Select,
    Json,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlSpec {
    #[serde(rename = "type")]
    pub kind: ControlKind,
    pub label: String,
    pub default: Value,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Only the leading comment can declare controls. This avoids interpreting JavaScript
/// strings, template literals or commented-out experiments as active UI metadata.
pub fn parse_controls(source: &str) -> Result<Controls, String> {
    let source = source.trim_start_matches('\u{feff}').trim_start();
    let Some(comment) = source.strip_prefix("/*") else {
        return Ok(Controls::new());
    };
    let Some(metadata) = comment.trim_start().strip_prefix("@controls") else {
        return Ok(Controls::new());
    };
    let end = metadata
        .find("*/")
        .ok_or("The @controls comment needs a closing */")?;
    let metadata = metadata[..end].trim();
    if metadata.len() > MAX_METADATA_BYTES {
        return Err("Experiment controls exceed 64 KiB".into());
    }
    let UniqueControls(controls) = serde_json::from_str(metadata)
        .map_err(|error| format!("Invalid @controls JSON: {error}"))?;
    if controls.len() > MAX_CONTROLS {
        return Err(format!(
            "An experiment can expose at most {MAX_CONTROLS} controls"
        ));
    }
    for (name, spec) in &controls {
        validate_spec(name, spec).map_err(|error| format!("Control '{name}': {error}"))?;
    }
    Ok(controls)
}

fn validate_spec(name: &str, spec: &ControlSpec) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || ["__proto__", "constructor", "prototype"].contains(&name)
    {
        return Err("use a nonempty parameter name of at most 128 bytes".into());
    }
    if spec.label.trim().is_empty() || spec.label.len() > 128 {
        return Err("label must contain 1–128 bytes".into());
    }
    if spec
        .description
        .as_ref()
        .is_some_and(|text| text.len() > 1024)
    {
        return Err("description must be at most 1024 bytes".into());
    }
    let numeric = matches!(spec.kind, ControlKind::Integer | ControlKind::Number);
    if !numeric && (spec.min.is_some() || spec.max.is_some() || spec.step.is_some()) {
        return Err("min, max and step apply only to numeric controls".into());
    }
    if [spec.min, spec.max, spec.step]
        .into_iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
    {
        return Err("numeric bounds must be finite and within ±1000000".into());
    }
    if spec.min.zip(spec.max).is_some_and(|(min, max)| min > max) {
        return Err("min cannot exceed max".into());
    }
    if spec.step.is_some_and(|step| step <= 0.0) {
        return Err("step must be greater than zero".into());
    }
    if spec.kind == ControlKind::Integer
        && [spec.min, spec.max, spec.step]
            .into_iter()
            .flatten()
            .any(|v| v.fract() != 0.0)
    {
        return Err("integer bounds and step must be whole numbers".into());
    }
    if spec.kind == ControlKind::Select {
        if spec.options.is_empty()
            || spec.options.len() > 64
            || spec.options.iter().any(|s| s.len() > 256)
        {
            return Err("select needs 1–64 options, each at most 256 bytes".into());
        }
        let unique: std::collections::HashSet<_> = spec.options.iter().collect();
        if unique.len() != spec.options.len() {
            return Err("select options must be unique".into());
        }
    } else if !spec.options.is_empty() {
        return Err("options apply only to select controls".into());
    }
    validate_value(spec, &spec.default).map_err(|error| format!("invalid default: {error}"))
}

pub(crate) fn validate_value(spec: &ControlSpec, value: &Value) -> Result<(), String> {
    match spec.kind {
        ControlKind::Integer | ControlKind::Number => {
            let number = value.as_f64().ok_or("expected a number")?;
            if !number.is_finite() || number.abs() > 1_000_000.0 {
                return Err("number must be finite and within ±1000000".into());
            }
            if spec.kind == ControlKind::Integer && number.fract() != 0.0 {
                return Err("expected a whole number".into());
            }
            if let Some(min) = spec.min {
                if number < min {
                    return Err(format!("must be at least {min}"));
                }
            }
            if let Some(max) = spec.max {
                if number > max {
                    return Err(format!("must be at most {max}"));
                }
            }
        }
        ControlKind::Boolean if !value.is_boolean() => return Err("expected true or false".into()),
        ControlKind::Text | ControlKind::Select | ControlKind::Color => {
            let text = value.as_str().ok_or("expected text")?;
            if text.len() > 16_384 {
                return Err("text exceeds 16 KiB".into());
            }
            if spec.kind == ControlKind::Select && !spec.options.iter().any(|option| option == text)
            {
                return Err(format!("choose one of: {}", spec.options.join(", ")));
            }
            if spec.kind == ControlKind::Color {
                crate::model::parse_color(text)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Undeclared fields are preserved for custom scripts and legacy project parameters.
pub fn validate_parameters(controls: &Controls, parameters: &Value) -> Result<(), String> {
    let values = parameters
        .as_object()
        .ok_or("Experiment parameters must be a JSON object")?;
    for (name, spec) in controls {
        if let Some(value) = values.get(name) {
            validate_value(spec, value).map_err(|error| format!("Parameter '{name}': {error}"))?;
        }
    }
    Ok(())
}

pub fn merge_defaults(controls: &Controls, parameters: &Value) -> Result<Value, String> {
    validate_parameters(controls, parameters)?;
    let mut merged: Map<String, Value> = controls
        .iter()
        .map(|(name, spec)| (name.clone(), spec.default.clone()))
        .collect();
    merged.extend(parameters.as_object().unwrap().clone());
    Ok(Value::Object(merged))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arbitrary_script_fields_defaults_and_legacy_extras() {
        let controls = parse_controls(r##"/* @controls {
            "branchDensity":{"type":"number","label":"Branch density","default":0.4,"min":0,"max":1},
            "tint":{"type":"color","label":"Colour","default":"#ffcc00aa"},
            "shape":{"type":"select","label":"Shape","default":"tree","options":["tree","mesh"]}
        } */ function build() {}"##).unwrap();
        let values =
            merge_defaults(&controls, &json!({"branchDensity":0.2,"anything":[1,2,3]})).unwrap();
        assert_eq!(
            values,
            json!({"branchDensity":0.2,"tint":"#ffcc00aa","shape":"tree","anything":[1,2,3]})
        );
        assert!(validate_parameters(&controls, &json!({"branchDensity":2}))
            .unwrap_err()
            .contains("branchDensity"));
        assert!(validate_parameters(&controls, &json!({"shape":"square"})).is_err());
        assert!(validate_parameters(&controls, &json!({"tint":"red"})).is_err());
    }

    #[test]
    fn metadata_is_optional_and_never_executes_source() {
        assert!(parse_controls("function build(graph) { while(true){} }")
            .unwrap()
            .is_empty());
        assert!(parse_controls("const text = '/* @controls malformed */';")
            .unwrap()
            .is_empty());
        assert_eq!(
            merge_defaults(&Controls::new(), &json!({"custom":true})).unwrap(),
            json!({"custom":true})
        );
    }

    #[test]
    fn invalid_metadata_and_values_are_explicit_errors() {
        for source in [
            "/* @controls { */",
            "/* @controls {}",
            "/* @controls [] */",
            r#"/* @controls {"n":{"type":"integer","label":"Nodes","default":2,"min":3}} */"#,
            r#"/* @controls {"n":{"type":"integer","label":"Nodes","default":2,"step":0}} */"#,
            r#"/* @controls {"n":{"type":"integer","label":"Nodes","default":2,"min":1.5}} */"#,
            r#"/* @controls {"n":{"type":"text","label":"Text","default":"x","surprise":true}} */"#,
            r#"/* @controls {"n":{"type":"integer","label":"N","default":1},"n":{"type":"integer","label":"Other N","default":2}} */"#,
        ] {
            assert!(parse_controls(source).is_err(), "{source}");
        }
        let controls = parse_controls(r#"/* @controls {"limit":{"type":"integer","label":"Limit","default":5,"min":1,"max":50}} */"#).unwrap();
        for params in [
            json!({"limit":1.5}),
            json!({"limit":"5"}),
            json!({"limit":100}),
            json!([]),
        ] {
            assert!(validate_parameters(&controls, &params).is_err());
        }
        assert!(parse_controls("function build(){}").unwrap().is_empty());
    }
}
