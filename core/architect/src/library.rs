//! A caller-supplied directory of authored graph documents with closed-set parameters (spec
//! D8). `<name>.yaml|json` is the document, in the authored format `load_graph` reads;
//! `<name>.template.json` is its sidecar, declaring `{id, summary, parameters}` where every
//! parameter is a closed set of values with the question the judge is asked. Only `{{name}}` in
//! string leaves is substituted; a value outside `options`, a missing parameter, or a leftover
//! placeholder refuses. There is no default directory and no bundled template: an absent or
//! empty library makes the decision step of `synthesize_with` a no-op.
//!
//! Nothing here validates a graph: a filled template enters the SAME chain a draft takes
//! (schema, lint, viability, stamp, allowlist), so the library can fill nothing the schema
//! would refuse and never widens the operator's allowlist.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::judgment::reuse::NO_TEMPLATE;
use crate::model::MAX_FIXTURE_BYTES;
use crate::refusal::ArchitectRefusal;

/// The most templates one library holds; a larger directory is refused, not truncated.
pub const MAX_TEMPLATES: usize = 64;
/// The suffix that marks a sidecar; the document is the file with the same stem.
pub const SIDECAR_SUFFIX: &str = ".template.json";

/// One closed-set parameter: the question the judge answers and the values it may pick, each
/// with the description the judge sees.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub question: String,
    pub options: BTreeMap<String, String>,
}

/// The sidecar as written: every key required, no other key admitted.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Sidecar {
    id: String,
    summary: String,
    parameters: BTreeMap<String, Parameter>,
}

/// One template: the sidecar's declaration and the document it fills.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub id: String,
    pub summary: String,
    pub parameters: BTreeMap<String, Parameter>,
    /// The authored document, placeholders included, as parsed.
    pub document: Value,
}

/// The loaded library: every template, sorted by file name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphLibrary {
    templates: Vec<Template>,
}

fn invalid(path: &str, message: impl Into<String>) -> ArchitectRefusal {
    ArchitectRefusal::LibraryInvalid {
        path: path.to_owned(),
        message: message.into(),
    }
}

/// The file name of `path`, for a refusal: never the directory it sits in.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || ".".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

impl GraphLibrary {
    /// Reads every `<name>.template.json` under `dir` (not recursively), sorted by name, each
    /// with its sibling `<name>.yaml` or `<name>.json`. A directory holding no sidecar is an
    /// empty library.
    ///
    /// # Errors
    /// [`ArchitectRefusal::LibraryInvalid`] naming the offending file when `dir` is not a
    /// directory or cannot be listed, a sidecar has no document, either file exceeds
    /// [`MAX_FIXTURE_BYTES`] or is not what its extension says, the sidecar carries an unknown
    /// key or an empty, reserved (`none`, the judge's "no template" answer) or duplicate `id`,
    /// the document is not a mapping, or there are more than [`MAX_TEMPLATES`] sidecars.
    pub fn load(dir: &Path) -> Result<Self, ArchitectRefusal> {
        let dir_name = file_name(dir);
        let metadata = std::fs::metadata(dir)
            .map_err(|_| invalid(&dir_name, "the library directory cannot be inspected"))?;
        if !metadata.is_dir() {
            return Err(invalid(&dir_name, "the library path is not a directory"));
        }
        let entries = std::fs::read_dir(dir)
            .map_err(|_| invalid(&dir_name, "the library directory cannot be listed"))?;
        let mut sidecars: Vec<String> = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|_| invalid(&dir_name, "the library directory cannot be listed"))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(SIDECAR_SUFFIX) {
                sidecars.push(name);
            }
        }
        sidecars.sort();
        if sidecars.len() > MAX_TEMPLATES {
            return Err(invalid(
                &dir_name,
                format!("the library holds more than {MAX_TEMPLATES} templates"),
            ));
        }
        let mut templates: Vec<Template> = Vec::with_capacity(sidecars.len());
        for sidecar_name in sidecars {
            let stem = &sidecar_name[..sidecar_name.len() - SIDECAR_SUFFIX.len()];
            let sidecar_bytes = read_bounded(&dir.join(&sidecar_name), &sidecar_name)?;
            let sidecar: Sidecar = serde_json::from_slice(&sidecar_bytes).map_err(|_| {
                invalid(
                    &sidecar_name,
                    "the sidecar is not {id, summary, parameters} with no other key",
                )
            })?;
            if sidecar.id.is_empty() {
                return Err(invalid(&sidecar_name, "the sidecar's id is empty"));
            }
            // `none` is the `template` answer that names no template, so a template with that
            // id could never be chosen: refuse it here rather than load it unreachable (#1126
            // review finding).
            if sidecar.id == NO_TEMPLATE {
                return Err(invalid(
                    &sidecar_name,
                    format!("the sidecar's id `{NO_TEMPLATE}` is reserved"),
                ));
            }
            if templates.iter().any(|known| known.id == sidecar.id) {
                return Err(invalid(&sidecar_name, "the sidecar's id is already taken"));
            }
            let document = read_document(dir, stem, &sidecar_name)?;
            templates.push(Template {
                id: sidecar.id,
                summary: sidecar.summary,
                parameters: sidecar.parameters,
                document,
            });
        }
        Ok(Self { templates })
    }

    /// Every template, in file-name order.
    #[must_use]
    pub fn templates(&self) -> &[Template] {
        &self.templates
    }

    /// The template whose sidecar declares `id`.
    #[must_use]
    pub fn template(&self, id: &str) -> Option<&Template> {
        self.templates.iter().find(|template| template.id == id)
    }
}

/// The document beside a sidecar: `<stem>.yaml` first, else `<stem>.json`; parsed by its
/// extension into a JSON value that must be a mapping.
fn read_document(dir: &Path, stem: &str, sidecar_name: &str) -> Result<Value, ArchitectRefusal> {
    let yaml_name = format!("{stem}.yaml");
    let json_name = format!("{stem}.json");
    let (name, is_yaml) = if dir.join(&yaml_name).is_file() {
        (yaml_name, true)
    } else if dir.join(&json_name).is_file() {
        (json_name, false)
    } else {
        return Err(invalid(
            sidecar_name,
            "the sidecar has no document beside it (<name>.yaml or <name>.json)",
        ));
    };
    let bytes = read_bounded(&dir.join(&name), &name)?;
    let document: Value = if is_yaml {
        serde_yaml_ng::from_slice(&bytes).map_err(|_| invalid(&name, "the document is not YAML"))?
    } else {
        serde_json::from_slice(&bytes).map_err(|_| invalid(&name, "the document is not JSON"))?
    };
    if !document.is_object() {
        return Err(invalid(&name, "the document is not a mapping"));
    }
    Ok(document)
}

/// Reads one regular file of at most [`MAX_FIXTURE_BYTES`], refusing before the read when the
/// size says it is larger.
fn read_bounded(path: &Path, name: &str) -> Result<Vec<u8>, ArchitectRefusal> {
    let metadata =
        std::fs::metadata(path).map_err(|_| invalid(name, "the file cannot be inspected"))?;
    if !metadata.is_file() {
        return Err(invalid(name, "not a regular file"));
    }
    if metadata.len() > MAX_FIXTURE_BYTES as u64 {
        return Err(invalid(name, "the file exceeds the 4 MiB limit"));
    }
    std::fs::read(path).map_err(|_| invalid(name, "the file cannot be read"))
}

impl Template {
    /// The document with every `{{name}}` in a string leaf replaced by `values[name]`.
    ///
    /// # Errors
    /// [`ArchitectRefusal::LibraryInvalid`] (naming the template id) when a declared parameter
    /// has no value, a value is not one of the parameter's options, `values` names a parameter
    /// the template does not declare (a stray key is a caller defect to see, #1126 review
    /// finding), or a `{{` survives the substitution: a placeholder no parameter declares is
    /// never handed on as text.
    pub fn fill(&self, values: &BTreeMap<String, String>) -> Result<Value, ArchitectRefusal> {
        if let Some(name) = values
            .keys()
            .find(|name| !self.parameters.contains_key(*name))
        {
            return Err(invalid(
                &self.id,
                format!("parameter {name} is not declared by the template"),
            ));
        }
        for (name, parameter) in &self.parameters {
            let value = values
                .get(name)
                .ok_or_else(|| invalid(&self.id, format!("parameter {name} has no value")))?;
            if !parameter.options.contains_key(value) {
                return Err(invalid(
                    &self.id,
                    format!("parameter {name} was given a value outside its options"),
                ));
            }
        }
        let mut document = self.document.clone();
        substitute(&mut document, values);
        if serde_json::to_string(&document).is_ok_and(|text| text.contains("{{")) {
            return Err(invalid(
                &self.id,
                "a placeholder was not declared as a parameter",
            ));
        }
        Ok(document)
    }
}

/// Replaces `{{name}}` in every string leaf; keys, numbers and booleans are never touched. Only
/// the names in `values` (checked against the parameters by the caller) are substituted, each
/// once, so a value that happens to spell another placeholder is text, not a second pass.
fn substitute(value: &mut Value, values: &BTreeMap<String, String>) {
    match value {
        Value::String(text) => {
            for (name, replacement) in values {
                let placeholder = format!("{{{{{name}}}}}");
                if text.contains(&placeholder) {
                    *text = text.replace(&placeholder, replacement);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute(item, values)),
        Value::Object(map) => map.values_mut().for_each(|item| substitute(item, values)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(document: Value, options: &[&str]) -> Template {
        Template {
            id: "t".to_owned(),
            summary: String::new(),
            parameters: BTreeMap::from([(
                "program".to_owned(),
                Parameter {
                    question: "q".to_owned(),
                    options: options
                        .iter()
                        .map(|option| ((*option).to_owned(), String::new()))
                        .collect(),
                },
            )]),
            document,
        }
    }

    #[test]
    fn a_placeholder_in_a_key_is_never_substituted_and_refuses() {
        let template = template(
            serde_json::json!({"{{program}}": "run {{program}}", "n": 1}),
            &["cargo"],
        );
        let values = BTreeMap::from([("program".to_owned(), "cargo".to_owned())]);
        assert!(matches!(
            template.fill(&values),
            Err(ArchitectRefusal::LibraryInvalid { path, .. }) if path == "t"
        ));
    }

    #[test]
    fn a_value_that_spells_a_placeholder_is_text_and_the_leftover_refuses() {
        let template = template(serde_json::json!({"a": "{{program}}"}), &["{{program}}"]);
        let values = BTreeMap::from([("program".to_owned(), "{{program}}".to_owned())]);
        assert!(template.fill(&values).is_err(), "never a second pass");
    }

    /// An undeclared KEY in `values` refuses (#1126 review finding: the contract is refusal
    /// both ways, a value outside the options and a parameter the template never declared);
    /// with only declared keys, a number or boolean leaf is untouched.
    #[test]
    fn an_undeclared_key_refuses_and_a_number_leaf_is_untouched() {
        let template = template(
            serde_json::json!({"a": "{{program}}", "n": 7, "b": true}),
            &["cargo"],
        );
        let stray = BTreeMap::from([
            ("program".to_owned(), "cargo".to_owned()),
            ("extra".to_owned(), "x".to_owned()),
        ]);
        match template.fill(&stray) {
            Err(ArchitectRefusal::LibraryInvalid { path, message }) => {
                assert_eq!(path, "t");
                assert!(message.contains("extra"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        let values = BTreeMap::from([("program".to_owned(), "cargo".to_owned())]);
        let filled = template.fill(&values).unwrap();
        assert_eq!(filled, serde_json::json!({"a": "cargo", "n": 7, "b": true}));
    }
}
