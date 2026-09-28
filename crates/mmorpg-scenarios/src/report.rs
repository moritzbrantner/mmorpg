//! Line-oriented scenario output shared by both runners.
//!
//! Every line exists in two renderings: a compact human digest and a JSON
//! object. Both are derived from the same event, so `--json` never reports a
//! different verdict than the text output.

use serde_json::{Map, Value};

pub struct Report {
    schema: &'static str,
    lines: Vec<(String, Map<String, Value>)>,
    failures: usize,
}

impl Report {
    pub(crate) fn new(schema: &'static str) -> Self {
        Self {
            schema,
            lines: Vec::new(),
            failures: 0,
        }
    }

    /// Appends one output line. `kind` becomes the JSON `type` field.
    pub(crate) fn line(&mut self, kind: &str, text: String, fields: Value) {
        let mut object = Map::new();
        object.insert("schema".into(), Value::from(self.schema));
        object.insert("type".into(), Value::from(kind));
        if let Value::Object(fields) = fields {
            object.extend(fields);
        }
        self.lines.push((text, object));
    }

    pub(crate) fn fail(&mut self) {
        self.failures += 1;
    }

    #[must_use]
    pub fn failures(&self) -> usize {
        self.failures
    }

    #[must_use]
    pub fn passed(&self) -> bool {
        self.failures == 0
    }

    #[must_use]
    pub fn to_text(&self) -> String {
        let mut output = String::new();
        for (text, _) in &self.lines {
            output.push_str(text);
            output.push('\n');
        }
        output
    }

    /// One compact JSON object per line. Keys are sorted, so output is stable.
    #[must_use]
    pub fn to_json_lines(&self) -> String {
        let mut output = String::new();
        for (_, object) in &self.lines {
            output.push_str(&Value::Object(object.clone()).to_string());
            output.push('\n');
        }
        output
    }
}

/// Converts `CamelCase` enum variant names from `Debug` into `snake_case`
/// rejection kinds, e.g. `StaleLease { .. }` becomes `stale_lease`.
pub(crate) fn snake_kind(debug: &str) -> String {
    let name = debug
        .split(|character: char| !character.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default();
    let mut kind = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                kind.push('_');
            }
            kind.push(character.to_ascii_lowercase());
        } else {
            kind.push(character);
        }
    }
    kind
}

/// Scenario and actor names become output tokens; keep them unambiguous.
pub(crate) fn validate_name(what: &str, value: &str) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{what} {value:?} must be 1-64 characters of [A-Za-z0-9_-]"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_variant_names_become_snake_case_kinds() {
        assert_eq!(snake_kind("StaleLease { zone_id: 1 }"), "stale_lease");
        assert_eq!(
            snake_kind("ZoneNotAssigned(ZoneId(1))"),
            "zone_not_assigned"
        );
        assert_eq!(snake_kind("Frozen"), "frozen");
    }

    #[test]
    fn names_are_restricted_to_output_safe_tokens() {
        assert!(validate_name("bot", "alice_2").is_ok());
        assert!(validate_name("bot", "").is_err());
        assert!(validate_name("bot", "a b").is_err());
    }
}
