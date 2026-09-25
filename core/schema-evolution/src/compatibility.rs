use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use crate::{
    CatalogResources, MAX_JSON_DEPTH, canonical_json,
    reference::{referenced_value, resolve_reference, resolved_schema_references},
};

const BREAKING_CODE: &str = "GHC003_BREAKING_CHANGE";
const ANNOTATION_CODE: &str = "GHC101_ANNOTATION_CHANGED";
const OPTIONAL_PROPERTY_CODE: &str = "GHC102_OPTIONAL_PROPERTY_ADDED";
const COMPATIBLE_CODE: &str = "GHC103_COMPATIBLE_CHANGE";

/// The most severe compatibility class found in a comparison.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityClass {
    #[default]
    Unchanged,
    Annotation,
    Compatible,
    Breaking,
}

/// The minimum semantic-version segment required by a compatibility class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemverImpact {
    #[default]
    None,
    Patch,
    Minor,
    Major,
}

/// A payload-safe explanation of one schema compatibility decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityChange {
    pub schema: String,
    pub code: String,
    pub pointer: String,
    pub baseline_summary: String,
    pub candidate_summary: String,
    pub class: CompatibilityClass,
    pub impact: SemverImpact,
}

/// Deterministic aggregate compatibility between two explicit catalogs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityReport {
    pub compatible: bool,
    pub class: CompatibilityClass,
    pub impact: SemverImpact,
    pub changes: Vec<CompatibilityChange>,
}

/// Compares already loaded schema catalogs without filesystem or network access.
#[must_use]
pub fn compare_catalogs(
    baseline: &CatalogResources,
    candidate: &CatalogResources,
) -> CompatibilityReport {
    let mut comparison = Comparison {
        baseline,
        candidate,
        changes: Vec::new(),
    };
    for (schema, document) in &candidate.schemas {
        comparison.validate_candidate_references(schema, document);
    }
    let schema_names = baseline
        .schemas
        .keys()
        .chain(candidate.schemas.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for schema in schema_names {
        match (
            baseline.schemas.get(&schema),
            candidate.schemas.get(&schema),
        ) {
            (Some(left), Some(right)) => {
                comparison.compare_value(&schema, &schema, left, right, "", 0)
            }
            (Some(_), None) => comparison.breaking(&schema, "/", "schema present", "schema absent"),
            (None, Some(_)) => comparison.compatible(&schema, "/", "schema absent", "schema added"),
            (None, None) => {}
        }
    }

    comparison.changes.sort_by(|left, right| {
        (&left.schema, &left.pointer, &left.code).cmp(&(&right.schema, &right.pointer, &right.code))
    });
    let class = comparison
        .changes
        .iter()
        .map(|change| change.class)
        .max()
        .unwrap_or_default();
    let impact = comparison
        .changes
        .iter()
        .map(|change| change.impact)
        .max()
        .unwrap_or_default();
    CompatibilityReport {
        compatible: class != CompatibilityClass::Breaking,
        class,
        impact,
        changes: comparison.changes,
    }
}

struct Comparison<'a> {
    baseline: &'a CatalogResources,
    candidate: &'a CatalogResources,
    changes: Vec<CompatibilityChange>,
}

impl Comparison<'_> {
    #[allow(clippy::too_many_arguments)]
    fn compare_value(
        &mut self,
        schema: &str,
        owner: &str,
        baseline: &Value,
        candidate: &Value,
        pointer: &str,
        depth: usize,
    ) {
        if baseline == candidate {
            return;
        }
        if depth > MAX_JSON_DEPTH {
            self.breaking(
                schema,
                display_pointer(pointer),
                "schema depth bounded",
                "schema depth unprovable",
            );
            return;
        }

        match (baseline, candidate) {
            (Value::Bool(false), Value::Bool(true)) => self.compatible(
                schema,
                display_pointer(pointer),
                "schema rejects all",
                "schema accepts all",
            ),
            (Value::Bool(false), Value::Object(_)) => self.compatible(
                schema,
                display_pointer(pointer),
                "schema rejects all",
                "schema accepts conditionally",
            ),
            (Value::Object(_), Value::Bool(true)) => self.compatible(
                schema,
                display_pointer(pointer),
                "schema accepts conditionally",
                "schema accepts all",
            ),
            (Value::Bool(true), Value::Bool(false)) => self.breaking(
                schema,
                display_pointer(pointer),
                "schema accepts all",
                "schema rejects all",
            ),
            (Value::Bool(true), Value::Object(_)) => self.breaking(
                schema,
                display_pointer(pointer),
                "schema accepts all",
                "schema accepts conditionally",
            ),
            (Value::Object(_), Value::Bool(false)) => self.breaking(
                schema,
                display_pointer(pointer),
                "schema accepts conditionally",
                "schema rejects all",
            ),
            (Value::Object(left), Value::Object(right)) => {
                let keys = left
                    .keys()
                    .chain(right.keys())
                    .cloned()
                    .collect::<BTreeSet<_>>();
                for keyword in keys {
                    let left_value = left.get(&keyword);
                    let right_value = right.get(&keyword);
                    if left_value == right_value {
                        continue;
                    }
                    let keyword_pointer = join_pointer(pointer, &keyword);
                    self.compare_keyword(
                        schema,
                        owner,
                        &keyword,
                        left_value,
                        right_value,
                        &keyword_pointer,
                        depth + 1,
                    );
                }
            }
            _ => self.breaking(
                schema,
                display_pointer(pointer),
                "schema form present",
                "schema form changed",
            ),
        }
    }

    fn validate_candidate_references(&mut self, schema: &str, document: &Value) {
        for invalid_pointer in resolved_schema_references(self.candidate, schema, document)
            .err()
            .unwrap_or_default()
        {
            self.breaking(
                schema,
                &invalid_pointer,
                "reference absent or unresolved",
                "reference unresolved",
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compare_keyword(
        &mut self,
        schema: &str,
        owner: &str,
        keyword: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
        depth: usize,
    ) {
        match keyword {
            "title" | "description" | "$comment" | "examples" | "default" | "deprecated"
            | "readOnly" | "writeOnly" => self.annotation(schema, pointer, baseline, candidate),
            // Version enforcement is a separate gate; this field cannot classify its own change.
            "x-graphhelm-schema-version" => {}
            "properties" => {
                self.compare_properties(schema, owner, baseline, candidate, pointer, depth)
            }
            "$defs" | "definitions" => {
                self.compare_schema_map(schema, owner, baseline, candidate, pointer, depth)
            }
            "required" => self.compare_required(schema, baseline, candidate, pointer),
            "type" | "enum" => self.compare_set(schema, keyword, baseline, candidate, pointer),
            "$id" => self.breaking(
                schema,
                pointer,
                "schema identity present",
                "schema identity changed",
            ),
            "$ref" => self.compare_ref(schema, owner, baseline, candidate, pointer),
            "const" => self.compare_const(schema, baseline, candidate, pointer),
            "minimum" | "exclusiveMinimum" | "minLength" | "minItems" => {
                self.compare_bound(schema, baseline, candidate, pointer, BoundDirection::Lower)
            }
            "maximum" | "exclusiveMaximum" | "maxLength" | "maxItems" => {
                self.compare_bound(schema, baseline, candidate, pointer, BoundDirection::Upper)
            }
            "pattern" | "format" => {
                self.compare_opaque_constraint(schema, keyword, baseline, candidate, pointer)
            }
            "additionalProperties" | "unevaluatedProperties" => {
                self.compare_tail_schema(schema, owner, baseline, candidate, pointer, depth)
            }
            "items" => self.compare_items(schema, owner, baseline, candidate, pointer, depth),
            "prefixItems" => {
                self.compare_prefix_items(schema, owner, baseline, candidate, pointer, depth)
            }
            "allOf" | "anyOf" | "oneOf" => {
                self.compare_composition(schema, owner, keyword, baseline, candidate, pointer)
            }
            "not" => self.compare_not(schema, baseline, candidate, pointer),
            _ => self.breaking(
                schema,
                pointer,
                "keyword unchanged or absent",
                "keyword change unproven",
            ),
        }
    }

    fn annotation(
        &mut self,
        schema: &str,
        pointer: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
    ) {
        self.push(
            schema,
            pointer,
            ANNOTATION_CODE,
            summary_presence(baseline, "annotation"),
            summary_presence(candidate, "annotation"),
            CompatibilityClass::Annotation,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn compare_properties(
        &mut self,
        schema: &str,
        owner: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
        depth: usize,
    ) {
        let Some(left) = baseline.and_then(Value::as_object) else {
            if baseline.is_none()
                && let Some(right) = candidate.and_then(Value::as_object)
            {
                for property in right.keys() {
                    self.optional_property(schema, &join_pointer(pointer, property));
                }
                return;
            }
            self.breaking(
                schema,
                pointer,
                "properties absent",
                "properties unprovable",
            );
            return;
        };
        let Some(right) = candidate.and_then(Value::as_object) else {
            if candidate.is_none() {
                for property in left.keys() {
                    self.breaking(
                        schema,
                        &join_pointer(pointer, property),
                        "property present",
                        "property removed",
                    );
                }
                return;
            }
            self.breaking(
                schema,
                pointer,
                "properties present",
                "properties unprovable",
            );
            return;
        };

        for property in left
            .keys()
            .chain(right.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
        {
            let property_pointer = join_pointer(pointer, &property);
            match (left.get(&property), right.get(&property)) {
                (Some(left_schema), Some(right_schema)) => self.compare_value(
                    schema,
                    owner,
                    left_schema,
                    right_schema,
                    &property_pointer,
                    depth,
                ),
                (Some(_), None) => self.breaking(
                    schema,
                    &property_pointer,
                    "property present",
                    "property removed",
                ),
                (None, Some(_)) => self.optional_property(schema, &property_pointer),
                (None, None) => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compare_schema_map(
        &mut self,
        schema: &str,
        owner: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
        depth: usize,
    ) {
        let left = baseline.and_then(Value::as_object);
        let right = candidate.and_then(Value::as_object);
        let (Some(left), Some(right)) = (left, right) else {
            self.breaking(
                schema,
                pointer,
                "schema definitions present",
                "schema definitions changed",
            );
            return;
        };
        for name in left
            .keys()
            .chain(right.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
        {
            let nested_pointer = join_pointer(pointer, &name);
            match (left.get(&name), right.get(&name)) {
                (Some(left_schema), Some(right_schema)) => self.compare_value(
                    schema,
                    owner,
                    left_schema,
                    right_schema,
                    &nested_pointer,
                    depth,
                ),
                (Some(_), None) => self.breaking(
                    schema,
                    &nested_pointer,
                    "schema definition present",
                    "schema definition removed",
                ),
                (None, Some(_)) => self.compatible(
                    schema,
                    &nested_pointer,
                    "schema definition absent",
                    "schema definition added",
                ),
                (None, None) => {}
            }
        }
    }

    fn compare_required(
        &mut self,
        schema: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
    ) {
        let Some(left) = string_set_or_empty(baseline) else {
            self.breaking(
                schema,
                pointer,
                "required set present",
                "required set unprovable",
            );
            return;
        };
        let Some(right) = string_set_or_empty(candidate) else {
            self.breaking(
                schema,
                pointer,
                "required set present",
                "required set unprovable",
            );
            return;
        };
        if left == right {
            return;
        }
        if left.is_subset(&right) && left != right {
            self.breaking(
                schema,
                pointer,
                "required set smaller",
                "required set expanded",
            );
        } else if right.is_subset(&left) && left != right {
            self.compatible(
                schema,
                pointer,
                "required set larger",
                "required set relaxed",
            );
        } else {
            self.breaking(
                schema,
                pointer,
                "required set changed",
                "required set ambiguous",
            );
        }
    }

    fn compare_set(
        &mut self,
        schema: &str,
        keyword: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
    ) {
        match (baseline, candidate) {
            (None, Some(_)) => {
                self.breaking(
                    schema,
                    pointer,
                    "validation set absent",
                    "validation set introduced",
                );
                return;
            }
            (Some(_), None) => {
                self.compatible(
                    schema,
                    pointer,
                    "validation set present",
                    "validation set removed",
                );
                return;
            }
            _ => {}
        }
        if keyword == "type" {
            self.compare_type_domain(schema, baseline, candidate, pointer);
            return;
        }
        let left = validation_set(keyword, baseline);
        let right = validation_set(keyword, candidate);
        let (Some(left), Some(right)) = (left, right) else {
            self.breaking(
                schema,
                pointer,
                "validation set present",
                "validation set unprovable",
            );
            return;
        };
        if left == right {
            return;
        }
        if left.is_subset(&right) && left != right {
            self.compatible(
                schema,
                pointer,
                set_summary(keyword, "narrower"),
                set_summary(keyword, "widened"),
            );
        } else if right.is_subset(&left) && left != right {
            self.breaking(
                schema,
                pointer,
                set_summary(keyword, "wider"),
                set_summary(keyword, "narrowed"),
            );
        } else {
            self.breaking(
                schema,
                pointer,
                set_summary(keyword, "changed"),
                set_summary(keyword, "ambiguous"),
            );
        }
    }

    fn compare_type_domain(
        &mut self,
        schema: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
    ) {
        let left = baseline.and_then(type_value_set);
        let right = candidate.and_then(type_value_set);
        let (Some(left), Some(right)) = (left, right) else {
            self.breaking(schema, pointer, "type set present", "type set unprovable");
            return;
        };
        let left_in_right = type_domain_is_subset(&left, &right);
        let right_in_left = type_domain_is_subset(&right, &left);
        if left_in_right && right_in_left {
            return;
        }
        if left_in_right {
            self.compatible(schema, pointer, "type set narrower", "type set widened");
        } else if right_in_left {
            self.breaking(schema, pointer, "type set wider", "type set narrowed");
        } else {
            self.breaking(schema, pointer, "type set changed", "type set ambiguous");
        }
    }

    fn compare_const(
        &mut self,
        schema: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
    ) {
        match (baseline, candidate) {
            (Some(_), None) => self.compatible(
                schema,
                pointer,
                "const constraint present",
                "const constraint removed",
            ),
            _ => self.breaking(
                schema,
                pointer,
                "const constraint absent or different",
                "const constraint added or changed",
            ),
        }
    }

    fn compare_bound(
        &mut self,
        schema: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
        direction: BoundDirection,
    ) {
        match (baseline, candidate) {
            (None, Some(_)) => self.breaking(schema, pointer, "bound absent", "bound introduced"),
            (Some(_), None) => self.compatible(schema, pointer, "bound present", "bound removed"),
            (Some(left), Some(right)) => {
                let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) else {
                    self.breaking(schema, pointer, "bound present", "bound unprovable");
                    return;
                };
                let relaxed = match direction {
                    BoundDirection::Lower => right < left,
                    BoundDirection::Upper => right > left,
                };
                if relaxed {
                    self.compatible(schema, pointer, "bound stricter", "bound relaxed");
                } else {
                    self.breaking(schema, pointer, "bound looser", "bound restricted");
                }
            }
            (None, None) => {}
        }
    }

    fn compare_opaque_constraint(
        &mut self,
        schema: &str,
        keyword: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
    ) {
        if baseline.is_some() && candidate.is_none() {
            self.compatible(
                schema,
                pointer,
                format!("{keyword} constraint present"),
                format!("{keyword} constraint removed"),
            );
        } else {
            self.breaking(
                schema,
                pointer,
                format!("{keyword} absent or different"),
                format!("{keyword} introduced or changed"),
            );
        }
    }

    fn compare_ref(
        &mut self,
        schema: &str,
        owner: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
    ) {
        let left = baseline
            .and_then(Value::as_str)
            .and_then(|reference| resolve_reference(self.baseline, owner, reference).ok());
        let right = candidate
            .and_then(Value::as_str)
            .and_then(|reference| resolve_reference(self.candidate, owner, reference).ok());
        match (baseline, candidate, left, right) {
            (Some(_), Some(_), Some(left), Some(right)) if left == right => {}
            (Some(_), None, Some(_), _) => self.breaking(
                schema,
                pointer,
                "reference constraint present",
                "reference binding removed",
            ),
            _ => self.breaking(
                schema,
                pointer,
                "reference absent or resolved",
                "reference changed or unresolved",
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compare_tail_schema(
        &mut self,
        schema: &str,
        owner: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
        depth: usize,
    ) {
        let left = baseline.unwrap_or(&Value::Bool(true));
        let right = candidate.unwrap_or(&Value::Bool(true));
        match (left, right) {
            (Value::Bool(false), Value::Bool(true) | Value::Object(_)) => self.compatible(
                schema,
                pointer,
                "extra properties rejected",
                "extra properties accepted conditionally",
            ),
            (Value::Object(_), Value::Bool(true)) => self.compatible(
                schema,
                pointer,
                "extra properties constrained",
                "extra properties accepted",
            ),
            (Value::Bool(true), Value::Bool(false) | Value::Object(_))
            | (Value::Object(_), Value::Bool(false)) => self.breaking(
                schema,
                pointer,
                "extra properties more permissive",
                "extra properties restricted",
            ),
            (Value::Object(_), Value::Object(_)) => {
                self.compare_value(schema, owner, left, right, pointer, depth)
            }
            _ => self.breaking(
                schema,
                pointer,
                "extra property policy present",
                "extra property policy unprovable",
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compare_items(
        &mut self,
        schema: &str,
        owner: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
        depth: usize,
    ) {
        match (baseline, candidate) {
            (None, Some(_)) => {
                self.breaking(schema, pointer, "items unconstrained", "items constrained")
            }
            (Some(_), None) => {
                self.compatible(schema, pointer, "items constrained", "items unconstrained")
            }
            (Some(left), Some(right)) => {
                self.compare_value(schema, owner, left, right, pointer, depth)
            }
            (None, None) => {}
        }
    }

    /// Tuple positions, compared BY POSITION -- #517.
    ///
    /// The keyword had no arm at all and fell to the default, so every change under it, down to a
    /// sentence of prose, priced as `GHC003_BREAKING_CHANGE`. That is not a strict reading of the
    /// keyword; it is the absence of one. The other half of this module already knew the keyword:
    /// `normalized_schema_value` walks `prefixItems` when it normalises. Only the comparator did
    /// not.
    ///
    /// WHY NOT `compare_composition`, which handles `allOf`/`anyOf`/`oneOf` and is the obvious
    /// copy: those are SETS -- reordering their branches changes nothing, and it compares them as
    /// a set. `prefixItems` is a LIST, where index 0 constrains the first element and nothing
    /// else. A set comparison would call a swap of two positions "unchanged", which is a real
    /// breaking change reported as none.
    ///
    /// Each position is then the same three-way question `compare_items` already answers for the
    /// tail, applied one index at a time: newly constrained is breaking, no longer constrained is
    /// compatible, and constrained on both sides recurses.
    fn compare_prefix_items(
        &mut self,
        schema: &str,
        owner: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
        depth: usize,
    ) {
        let (left, right) = match (baseline, candidate) {
            (None, Some(_)) => {
                return self.breaking(
                    schema,
                    pointer,
                    "tuple positions unconstrained",
                    "tuple positions constrained",
                );
            }
            (Some(_), None) => {
                return self.compatible(
                    schema,
                    pointer,
                    "tuple positions constrained",
                    "tuple positions unconstrained",
                );
            }
            (Some(left), Some(right)) => (left, right),
            (None, None) => return,
        };
        // A non-array `prefixItems` is not a tuple declaration this checker can read. It keeps the
        // old conservative answer rather than silently comparing nothing: an unreadable shape must
        // not become the quietest possible verdict.
        let (Some(left), Some(right)) = (left.as_array(), right.as_array()) else {
            return self.breaking(
                schema,
                pointer,
                "tuple positions listed",
                "tuple positions not a list",
            );
        };
        for index in 0..left.len().max(right.len()) {
            let position = join_pointer(pointer, &index.to_string());
            match (left.get(index), right.get(index)) {
                (Some(from), Some(to)) => {
                    self.compare_value(schema, owner, from, to, &position, depth)
                }
                (Some(_), None) => self.compatible(
                    schema,
                    &position,
                    "position constrained",
                    "position unconstrained",
                ),
                (None, Some(_)) => self.breaking(
                    schema,
                    &position,
                    "position unconstrained",
                    "position constrained",
                ),
                (None, None) => {}
            }
        }
    }

    fn compare_composition(
        &mut self,
        schema: &str,
        owner: &str,
        keyword: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
    ) {
        match (baseline, candidate) {
            (None, Some(_)) => {
                self.breaking(
                    schema,
                    pointer,
                    "composition absent",
                    "composition introduced",
                );
                return;
            }
            (Some(_), None) => {
                self.compatible(
                    schema,
                    pointer,
                    "composition present",
                    "composition removed",
                );
                return;
            }
            _ => {}
        }
        let left = composition_set(self.baseline, owner, baseline);
        let right = composition_set(self.candidate, owner, candidate);
        let (Some(left), Some(right)) = (left, right) else {
            self.breaking(
                schema,
                pointer,
                "composition present",
                "composition change unprovable",
            );
            return;
        };
        if left == right
            && baseline.and_then(Value::as_array).map(Vec::len)
                == candidate.and_then(Value::as_array).map(Vec::len)
        {
            return;
        }
        let widening = if keyword == "allOf" {
            right.is_subset(&left)
        } else {
            left.is_subset(&right)
        };
        let narrowing = if keyword == "allOf" {
            left.is_subset(&right)
        } else {
            right.is_subset(&left)
        };
        if widening && left != right {
            if keyword == "oneOf"
                && !one_of_additions_are_disjoint(
                    self.baseline,
                    self.candidate,
                    owner,
                    baseline,
                    candidate,
                    &left,
                    pointer,
                )
            {
                self.breaking(
                    schema,
                    pointer,
                    "oneOf branches present",
                    "oneOf overlap unprovable",
                );
                return;
            }
            self.compatible(
                schema,
                pointer,
                "composition narrower",
                "composition widened",
            );
        } else if narrowing && left != right {
            self.breaking(schema, pointer, "composition wider", "composition narrowed");
        } else {
            self.breaking(
                schema,
                pointer,
                "composition changed",
                "composition ambiguous",
            );
        }
    }

    fn compare_not(
        &mut self,
        schema: &str,
        baseline: Option<&Value>,
        candidate: Option<&Value>,
        pointer: &str,
    ) {
        if baseline.is_some() && candidate.is_none() {
            self.compatible(
                schema,
                pointer,
                "negation constraint present",
                "negation constraint removed",
            );
        } else {
            self.breaking(
                schema,
                pointer,
                "negation absent or different",
                "negation introduced or changed",
            );
        }
    }

    fn optional_property(&mut self, schema: &str, pointer: &str) {
        self.push(
            schema,
            pointer,
            OPTIONAL_PROPERTY_CODE,
            "property absent",
            "optional property added",
            CompatibilityClass::Compatible,
        );
    }

    fn compatible(
        &mut self,
        schema: &str,
        pointer: &str,
        baseline_summary: impl Into<String>,
        candidate_summary: impl Into<String>,
    ) {
        self.push(
            schema,
            pointer,
            COMPATIBLE_CODE,
            baseline_summary,
            candidate_summary,
            CompatibilityClass::Compatible,
        );
    }

    fn breaking(
        &mut self,
        schema: &str,
        pointer: &str,
        baseline_summary: impl Into<String>,
        candidate_summary: impl Into<String>,
    ) {
        self.push(
            schema,
            pointer,
            BREAKING_CODE,
            baseline_summary,
            candidate_summary,
            CompatibilityClass::Breaking,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        schema: &str,
        pointer: &str,
        code: &str,
        baseline_summary: impl Into<String>,
        candidate_summary: impl Into<String>,
        class: CompatibilityClass,
    ) {
        if self.changes.iter().any(|change| {
            change.schema == schema && change.pointer == pointer && change.code == code
        }) {
            return;
        }
        let impact = match class {
            CompatibilityClass::Unchanged => SemverImpact::None,
            CompatibilityClass::Annotation => SemverImpact::Patch,
            CompatibilityClass::Compatible => SemverImpact::Minor,
            CompatibilityClass::Breaking => SemverImpact::Major,
        };
        self.changes.push(CompatibilityChange {
            schema: schema.to_owned(),
            code: code.to_owned(),
            pointer: display_pointer(pointer).to_owned(),
            baseline_summary: baseline_summary.into(),
            candidate_summary: candidate_summary.into(),
            class,
            impact,
        });
    }
}

#[derive(Clone, Copy)]
enum BoundDirection {
    Lower,
    Upper,
}

fn string_set_or_empty(value: Option<&Value>) -> Option<BTreeSet<String>> {
    match value {
        None => Some(BTreeSet::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| value.as_str().map(ToOwned::to_owned))
            .collect(),
        Some(_) => None,
    }
}

fn validation_set(keyword: &str, value: Option<&Value>) -> Option<BTreeSet<Vec<u8>>> {
    match value {
        None => None,
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| canonical_json(value).ok())
            .collect(),
        Some(value) if keyword == "type" => Some(BTreeSet::from([canonical_json(value).ok()?])),
        Some(_) => None,
    }
}

fn composition_set(
    resources: &CatalogResources,
    owner: &str,
    value: Option<&Value>,
) -> Option<BTreeSet<Vec<u8>>> {
    let values = value?.as_array()?;
    values
        .iter()
        .map(|value| normalized_schema_bytes(resources, owner, value, 0))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn one_of_additions_are_disjoint(
    baseline_resources: &CatalogResources,
    candidate_resources: &CatalogResources,
    owner: &str,
    baseline: Option<&Value>,
    candidate: Option<&Value>,
    baseline_fingerprints: &BTreeSet<Vec<u8>>,
    pointer: &str,
) -> bool {
    let Some(baseline_branches) = baseline.and_then(Value::as_array) else {
        return false;
    };
    let Some(candidate_branches) = candidate.and_then(Value::as_array) else {
        return false;
    };
    // D-049's fourth widening, gated on the fourth widening's own precondition: a propagated
    // constraint is sound only when it comes from the schema object IMMEDIATELY ENCLOSING this
    // exact `oneOf`, in both baseline and candidate. `pointer == "/oneOf"` is that check -- it is
    // true only for the document's OWN root combinator, the one JSON Schema's implicit AND binds
    // to that document's own sibling `type` and `required` keywords. Anywhere else (nested under a
    // `$defs` entry, inside `allOf`, ...) the enclosing object is not the document root and this
    // argument does not hold, so `nested_discriminators_are_disjoint` is never reached from there.
    let top_level = pointer == "/oneOf";
    candidate_branches.iter().all(|candidate_branch| {
        let Some(fingerprint) =
            normalized_schema_bytes(candidate_resources, owner, candidate_branch, 0)
        else {
            return false;
        };
        baseline_fingerprints.contains(&fingerprint)
            || baseline_branches.iter().all(|baseline_branch| {
                normalized_schema_bytes(baseline_resources, owner, baseline_branch, 0).is_some()
                    && (branches_are_provably_disjoint(baseline_branch, candidate_branch)
                        || (top_level
                            && nested_discriminators_are_disjoint(
                                baseline_resources,
                                candidate_resources,
                                owner,
                                baseline_branch,
                                candidate_branch,
                            )))
            })
    })
}

/// D-049's reopening: `discriminators_are_disjoint` extended one `$ref` hop for a discriminator an
/// ENCLOSING document makes required rather than each branch declaring it locally.
///
/// `event-envelope`'s top-level union is the case this exists for: 44 branches with no `type` and
/// no `required` of their own, whose disjointness is really established by a conjunction of three
/// separate places -- the document's own sibling `"type": "object"`, its sibling `required` naming
/// `kind`, and `kind`'s `$ref` to a SEPARATE, already well-formed `oneOf` (`$defs/eventKind`) whose
/// every branch requires `type`. JSON Schema's implicit AND binds a `oneOf` to its own enclosing
/// schema's other keywords even though no branch repeats them, so treating `kind` as required (from
/// the document's own `required` array) and `kind.type` as required (because EVERY branch of the
/// union `kind` resolves to agrees on it) is sound without editing any of the 44 branches.
///
/// Kept as a SEPARATE function from `discriminators_are_disjoint` rather than folded into it,
/// because the precondition is different in kind, not degree: the flat proof reads two branches in
/// isolation, this one reads the document each branch sits inside, and mixing the two would hide
/// which proof actually fired when one of them is later found unsound.
fn nested_discriminators_are_disjoint(
    baseline_resources: &CatalogResources,
    candidate_resources: &CatalogResources,
    owner: &str,
    left: &Value,
    right: &Value,
) -> bool {
    let Some(left_pairs) = document_required_nested_pairs(baseline_resources, owner) else {
        return false;
    };
    let Some(right_pairs) = document_required_nested_pairs(candidate_resources, owner) else {
        return false;
    };
    // Both sides, not either: D-049 names this explicitly ("in both baseline and candidate"). A
    // constraint the CANDIDATE document dropped (say, a future PR un-requiring `kind`) must not
    // keep certifying disjointness on the strength of what the baseline alone still says.
    left_pairs.intersection(&right_pairs).any(|(outer, inner)| {
        let path = [outer.as_str(), inner.as_str()];
        match (const_at_path(left, &path), const_at_path(right, &path)) {
            // Same restriction as `discriminators_are_disjoint`: STRING consts only, since JSON
            // Schema's `const` compares by value (`1` == `1.0`) where `serde_json::Number`'s
            // `PartialEq` does not agree, and a tagged union tags with strings regardless.
            (Some(Value::String(left_tag)), Some(Value::String(right_tag))) => {
                left_tag != right_tag
            }
            _ => false,
        }
    })
}

/// `(outer, inner)` name pairs a document's own shape makes globally required for any `oneOf`
/// branch enumerated at ITS root -- found structurally, not by name, so this reads any document
/// with the same shape rather than one hardcoded to `kind`/`type`.
///
/// `None` when the document is not object-only at its root: that is the other half of the
/// implicit-AND argument this proof rests on. A non-object instance matches no branch's
/// `properties`/`required` at all, object or not, so the propagated `type` must hold before any
/// propagated `required` name means anything.
///
/// A pair `(outer, inner)` qualifies when `outer` is in the document's own top-level `required`
/// array (so every instance of the document carries it), `properties.<outer>` is a `$ref` to some
/// OTHER schema, that schema CONSTRAINS ITS OWN INSTANCES TO OBJECTS, and it is itself a `oneOf`
/// whose branches EVERY ONE locally requires `inner`. The requiredness half is
/// `discriminators_are_disjoint`'s own soundness argument applied one level up: if every branch of
/// a union requires a name, that name is required by any instance satisfying the union, independent
/// of which branch actually matched.
///
/// **The object-only half is a SEPARATE link this proof needs and is not a restatement of the
/// caller's own `top_level` check.** `top_level` establishes that the OUTER document -- `kind`'s
/// container -- accepts only objects; it says nothing about what `kind`'s OWN value may be.
/// `properties.<name>.const` and `required` are no-ops on a non-object instance (the same #627
/// argument `branch_is_object_only` exists for at the flat level), so unless the value at `kind` is
/// itself constrained to `object`, an instance where `kind` is a bare string satisfies every branch
/// of `eventKind` whose `required: ["type"]` it trivially clears, and `kind.type.const` constrains
/// nothing. Accepted when the referenced schema itself declares `"type": "object"`, OR when every
/// one of its `oneOf` branches does -- the same closure `common` is already built from, so checking
/// it costs one more pass over branches already in hand, not a new one (K's review of #758, finding
/// the flat proof's own `object_only` was never propagated to this reference hop: today's 44
/// `eventKind` branches all happen to declare `"type": "object"`, but `eventKind` itself does not
/// REQUIRE it, so a 45th branch omitting it would reopen #627's hole here with nothing to catch it
/// -- `branches_are_provably_disjoint`'s own `object_only` check is bypassed on this path, since
/// `nested_discriminators_are_disjoint` is an ALTERNATIVE proof, not an addition to it).
fn document_required_nested_pairs(
    resources: &CatalogResources,
    owner: &str,
) -> Option<BTreeSet<(String, String)>> {
    let document = resources.schemas.get(owner)?;
    if document.get("type").and_then(Value::as_str) != Some("object") {
        return None;
    }
    let required = document.get("required").and_then(Value::as_array)?;
    let properties = document.get("properties").and_then(Value::as_object)?;
    let mut pairs = BTreeSet::new();
    for outer in required.iter().filter_map(Value::as_str) {
        let Some(reference) = properties
            .get(outer)
            .and_then(|property_schema| property_schema.get("$ref"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Ok(resolved) = resolve_reference(resources, owner, reference) else {
            continue;
        };
        let Ok((_, target)) = referenced_value(resources, &resolved) else {
            continue;
        };
        let Some(branches) = target.get("oneOf").and_then(Value::as_array) else {
            continue;
        };
        if branches.is_empty() {
            continue;
        }
        // LINK 2 (K's review of #758): the referenced union must itself be constrained to
        // objects, either at its own root or unanimously by its branches, or `kind.type.const`
        // constrains nothing on a `kind` value that is not an object at all.
        //
        // ONE READER FOR BOTH HALVES. The first version asked `target.get("type").as_str() ==
        // Some("object")` here and `branch_is_object_only` there -- two answers to one question,
        // and only the second complete: `as_str` returns None for the ARRAY spelling
        // `"type": ["object"]`, which is valid JSON Schema and exactly equivalent, while
        // `branch_is_object_only` goes through `type_value_set` and handles both. A target is a
        // schema object like any branch, so it reads with the same function and the duplication
        // goes with it. The gap was latent -- no schema under `schemas/` uses the array form
        // today, and its direction was a false REFUSAL rather than a false certification -- but it
        // was the shape this change exists to correct, one line above the correction.
        // (Found by an adversarial re-read that asked what the FILE already knows how to do that
        // the CHANGE does not, rather than whether each stated link is sound.)
        //
        // DECLARED LIMIT, inherited rather than introduced: `allOf: [{"type": "object"}]` and a
        // `$ref` resolving to an object constraint are missed here too, because
        // `branch_is_object_only` misses them. That is a property of this file's type reader, and
        // it fails toward refusing a certification rather than granting one.
        let target_object_only =
            branch_is_object_only(target) || branches.iter().all(branch_is_object_only);
        if !target_object_only {
            continue;
        }
        let mut common: Option<BTreeSet<&str>> = None;
        for branch in branches {
            let names = required_property_names(branch);
            common = Some(match common {
                None => names,
                Some(existing) => existing.intersection(&names).copied().collect(),
            });
        }
        pairs.extend(
            common
                .into_iter()
                .flatten()
                .map(|inner| (outer.to_owned(), inner.to_owned())),
        );
    }
    Some(pairs)
}

/// The constant a branch pins at a nested `properties` path, if it pins one at all -- the general
/// reader `const_of_property` is the depth-1 case of. Walks `properties` keyword pairs down the
/// given path (`["kind", "type"]` reads `branch.properties.kind.properties.type.const`), which is
/// what a discriminator two `properties` hops deep looks like once resolved through no `$ref` at
/// all: the top-level `oneOf` branches in `event-envelope` write `kind`'s nested shape inline.
fn const_at_path<'a>(branch: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = branch.get("properties")?.as_object()?.get(*first)?;
    for name in rest {
        current = current.get("properties")?.as_object()?.get(*name)?;
    }
    current.get("const")
}

/// Nothing can satisfy both branches, by either proof this module can carry out.
///
/// Two independent arguments, and the order is deliberate: the tagged-union proof runs first
/// because it is the one that ANSWERS for the shape this repository actually writes. Type sets
/// decide only when the branches differ at the top level, and every branch of a tagged union is
/// `"object"`, so on its own that test is silent exactly where the schemas need it (#626).
/// Whether EVERY pair of branches in a `oneOf` is provably disjoint.
///
/// The same question `one_of_additions_are_disjoint` asks of an addition, asked of a union as it
/// stands — exposed because the set of unions this prover CANNOT decide is a property the gate pins
/// (D-049). A blind spot nobody enumerates grows in silence; one that is pinned fires both when it
/// grows and when it becomes stale.
///
/// Answers only about the branches handed to it. A union whose branches reach their constraints
/// through `$ref` or through an enclosing schema reads as unprovable here, which is the honest
/// answer for a prover that deliberately looks at neither.
#[must_use]
pub fn one_of_branches_are_provably_disjoint(branches: &[Value]) -> bool {
    branches.iter().enumerate().all(|(index, left)| {
        branches[index + 1..]
            .iter()
            .all(|right| branches_are_provably_disjoint(left, right))
    })
}

fn branches_are_provably_disjoint(left: &Value, right: &Value) -> bool {
    discriminators_are_disjoint(left, right) || branch_types_are_disjoint(left, right)
}

/// The tagged-union proof: one property both branches REQUIRE and each pins to a DIFFERENT const.
///
/// **`required` is load-bearing and is not a formality.** A property that both branches pin to a
/// different constant but neither demands proves nothing at all: an instance carrying no such
/// property satisfies both, the branches overlap, and a `oneOf` that must match exactly one really
/// is broken by the addition. Comparing the consts alone would turn today's FALSE BREAKING into a
/// false COMPATIBLE -- silent, and strictly worse, because a breaking change classified as
/// breaking still gets a version bump while one classified as compatible gets nothing and nothing
/// downstream re-checks it. Measured against the corpus this exists for: all 44 branches of
/// `event-envelope`'s `$defs/eventKind` and both of `$defs/clearanceVerifier` require their tag,
/// so the sound rule costs nothing that the lax one would have bought.
///
/// **And the tag must be a STRING**, for a second soundness reason found by reading rather than by
/// a failing test: JSON Schema compares `const` by value, where `1` and `1.0` are the same number,
/// while `serde_json`'s `PartialEq` on `Number` does not agree. Two branches pinning `1` and `1.0`
/// would read as different tags and are not. Tagged unions tag with strings, so the restriction
/// costs nothing real and removes the ambiguity outright.
///
/// **And both branches must be object-only.** `required` and `properties` say nothing about a
/// non-object instance, so two branches that omit `"type": "object"` -- or admit anything besides
/// it -- overlap on every non-object value no matter how distinct their tags read. That was a real
/// hole in the first version of this function, found in review, and it is the third restriction of
/// the same family as the two above: free against this corpus, and invisible to the test the fix
/// was written for.
///
/// Deliberately NOT generalised beyond this: no `enum` of one value, no `allOf` flattening, no
/// looking through `$ref`. Each would be a separate proof with its own soundness argument, and an
/// unsound widening here fails in the silent direction.
fn discriminators_are_disjoint(left: &Value, right: &Value) -> bool {
    let (Some(left_branch), Some(right_branch)) = (left.as_object(), right.as_object()) else {
        return false;
    };
    // BOTH branches must accept objects and NOTHING ELSE, and this is the restriction the whole
    // argument rests on rather than a tidiness check. `required` and `properties` are NO-OPS for a
    // non-object instance: a branch that does not say `"type": "object"` accepts the number `42`,
    // and so does its sibling, so the two overlap on every non-object value while their tags look
    // perfectly distinct. The discriminator argument reasons about properties an instance need not
    // carry at all (Codex P1 on PR #627).
    //
    // Exactly `{object}`, not "contains object": a branch typed `["object", "string"]` overlaps its
    // sibling on every string for the same reason.
    if !branch_is_object_only(left) || !branch_is_object_only(right) {
        return false;
    }
    let shared_requirements: BTreeSet<&str> = required_property_names(left)
        .intersection(&required_property_names(right))
        .copied()
        .collect();
    shared_requirements.iter().any(|name| {
        match (
            const_of_property(left_branch, name),
            const_of_property(right_branch, name),
        ) {
            // STRING consts only, and this is the second place `required` is: a narrowing that
            // fails safe. JSON Schema compares `const` by VALUE, where `1` and `1.0` are the same
            // number, while `serde_json`'s `PartialEq` on `Number` does not say so. Two branches
            // pinning `1` and `1.0` would be "different" here and are not, which is the silent
            // direction. A tagged union tags with strings; anything else stays unprovable.
            (Some(Value::String(left_tag)), Some(Value::String(right_tag))) => {
                left_tag != right_tag
            }
            _ => false,
        }
    })
}

/// The names a branch demands. A branch with no `required` array demands nothing.
fn required_property_names(branch: &Value) -> BTreeSet<&str> {
    branch
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// The constant a branch pins one of its properties to, if it pins it at all.
fn const_of_property<'a>(
    branch: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Option<&'a Value> {
    branch
        .get("properties")?
        .as_object()?
        .get(name)?
        .get("const")
}

/// Exactly `{"object"}`, not "contains object" -- shared by `discriminators_are_disjoint` (each
/// branch, locally) and `document_required_nested_pairs` (the inner union's branches, one `$ref`
/// hop out). A branch typed `["object", "string"]`, or untyped, says nothing about a non-object
/// instance, so `required`/`properties` are no-ops for it and it overlaps its sibling on every
/// value outside `object` no matter how distinct their tags read (Codex P1 on PR #627).
fn branch_is_object_only(branch: &Value) -> bool {
    branch_type_set(branch).is_some_and(|types| types.len() == 1 && types.contains("object"))
}

fn branch_types_are_disjoint(left: &Value, right: &Value) -> bool {
    let Some(left_types) = branch_type_set(left) else {
        return false;
    };
    let Some(right_types) = branch_type_set(right) else {
        return false;
    };
    left_types.iter().all(|left_type| {
        right_types.iter().all(|right_type| {
            left_type != right_type
                && !matches!(
                    (left_type.as_str(), right_type.as_str()),
                    ("integer", "number") | ("number", "integer")
                )
        })
    })
}

fn branch_type_set(value: &Value) -> Option<BTreeSet<String>> {
    type_value_set(value.as_object()?.get("type")?)
}

fn type_value_set(value: &Value) -> Option<BTreeSet<String>> {
    match value {
        Value::String(value) => Some(BTreeSet::from([value.clone()])),
        Value::Array(values) => values
            .iter()
            .map(|value| value.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => None,
    }
}

fn type_domain_is_subset(left: &BTreeSet<String>, right: &BTreeSet<String>) -> bool {
    left.iter().all(|left_type| {
        right.contains(left_type) || (left_type == "integer" && right.contains("number"))
    })
}

fn normalized_schema_bytes(
    resources: &CatalogResources,
    owner: &str,
    value: &Value,
    depth: usize,
) -> Option<Vec<u8>> {
    let normalized = normalized_schema_value(resources, owner, value, depth)?;
    canonical_json(&normalized).ok()
}

fn normalized_schema_value(
    resources: &CatalogResources,
    owner: &str,
    value: &Value,
    depth: usize,
) -> Option<Value> {
    if depth > MAX_JSON_DEPTH {
        return None;
    }
    let Some(values) = value.as_object() else {
        return Some(value.clone());
    };
    let mut normalized = values.clone();

    if let Some(reference) = values.get("$ref") {
        normalized.insert(
            "$ref".into(),
            Value::String(resolve_reference(resources, owner, reference.as_str()?).ok()?),
        );
    }

    for keyword in [
        "properties",
        "patternProperties",
        "$defs",
        "definitions",
        "dependentSchemas",
    ] {
        let Some(children) = values.get(keyword).and_then(Value::as_object) else {
            continue;
        };
        let mut normalized_children = children.clone();
        for (name, child) in children {
            normalized_children.insert(
                name.clone(),
                normalized_schema_value(resources, owner, child, depth + 1)?,
            );
        }
        normalized.insert(keyword.into(), Value::Object(normalized_children));
    }

    for keyword in [
        "additionalProperties",
        "unevaluatedProperties",
        "unevaluatedItems",
        "propertyNames",
        "items",
        "contains",
        "not",
        "if",
        "then",
        "else",
    ] {
        if let Some(child) = values.get(keyword) {
            normalized.insert(
                keyword.into(),
                normalized_schema_value(resources, owner, child, depth + 1)?,
            );
        }
    }

    for keyword in ["prefixItems", "allOf", "anyOf", "oneOf"] {
        if let Some(children) = values.get(keyword).and_then(Value::as_array) {
            let normalized_children = children
                .iter()
                .map(|child| normalized_schema_value(resources, owner, child, depth + 1))
                .collect::<Option<Vec<_>>>()?;
            normalized.insert(keyword.into(), Value::Array(normalized_children));
        }
    }

    Some(Value::Object(normalized))
}

fn summary_presence(value: Option<&Value>, label: &str) -> String {
    if value.is_some() {
        format!("{label} present")
    } else {
        format!("{label} absent")
    }
}

fn set_summary(keyword: &str, state: &str) -> String {
    format!("{keyword} set {state}")
}

fn display_pointer(pointer: &str) -> &str {
    if pointer.is_empty() { "/" } else { pointer }
}

fn join_pointer(pointer: &str, segment: &str) -> String {
    format!("{pointer}/{}", escape_pointer(segment))
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
