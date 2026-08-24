//! Deterministic resolution of layered code rules (task-002, #218).
//!
//! The resolver RECEIVES its source set as validated envelopes and never discovers it, and it
//! RECEIVES the clock rather than reading one. The acceptance criterion fixes inputs, clock,
//! scopes and snapshots; a resolver that walked the filesystem would let readdir order change the
//! result without changing any of the four, and one that called `Utc::now()` would change the
//! result with nothing changing at all.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use graphhelm_protocols::{DevelopmentEnvelope, DevelopmentKind};

/// The rule-specific payload carried in a `CodeRule` envelope's `spec`.
///
/// Named for what it is. The wire kind `CodeRule` is the whole ENVELOPE -- `apiVersion`, `kind`,
/// `metadata`, `producer`, `spec`, `digest` -- as the package's own
/// `fixtures/contracts/valid/code-rule-minimal.json` fixes it. An earlier draft of this module
/// called a bare `{rule_id, conflict_key, minimum}` struct `CodeRule`, which is the parallel
/// vocabulary that fixture's own statement warns against.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeRuleSpec {
    /// The key this rule's requirement competes on.
    pub conflict_key: String,
    /// The requirement itself. One operator (`minimum`) in this slice.
    pub minimum: u32,
    /// The registered compatibility operator for this key. Closed set.
    #[serde(default = "default_operator")]
    pub operator: String,
    /// structural | quality | preference.
    #[serde(default = "default_enforcement")]
    pub enforcement: String,
    /// Predicate map. An empty selector matches everything.
    #[serde(default)]
    pub selector: BTreeMap<String, String>,
    /// The owner override, when one is asserted.
    #[serde(default)]
    pub waiver: Option<serde_json::Value>,
    /// Optional end of the validity window.
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,
}

fn default_operator() -> String {
    "minimum".to_owned()
}

fn default_enforcement() -> String {
    "structural".to_owned()
}

/// Every field a complete owner override must carry, in the order the design lists them.
///
/// Ordered and named so the refusal can say WHICH field is missing. A bare "invalid waiver" sends
/// the owner back to diffing their override against a spec, which is the cost #247 records.
const REQUIRED_WAIVER_FIELDS: [&str; 6] = [
    "actor",
    "reason",
    "acknowledgedRisks",
    "affectedContractVersion",
    "affectedGraphVersion",
    "resultStatus",
];

/// How two selectors relate. FOUR answers, not two.
///
/// The failure mode this type exists to prevent is a comparator that always returns an ordering,
/// because a total order is easier to sort with. `Incomparable` and `Equal` are different facts:
/// equal selectors describe the SAME population and neither adds a predicate, while incomparable
/// selectors describe populations neither of which contains the other. Collapsing either into a
/// boolean lets one dimension silently outrank another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dominance {
    /// The left selector is strictly more specific.
    Dominates,
    /// The right selector is strictly more specific.
    DominatedBy,
    /// The same predicates with the same values.
    Equal,
    /// Neither contains the other.
    Incomparable,
}

/// Selector dominance, from the two predicate maps.
///
/// Dominance is a PARTIAL order and the four answers are the point. `left` dominates `right` only
/// when every predicate in `right` appears in `left` with the SAME VALUE and `left` adds at least
/// one. A shared dimension holding different values makes the two describe disjoint populations,
/// so neither can strengthen the other -- that is `Incomparable`, and counting predicates would
/// have called it `Equal`.
#[must_use]
pub fn dominance(left: &serde_json::Value, right: &serde_json::Value) -> Dominance {
    let left = predicates(left);
    let right = predicates(right);

    let left_contains_right = right
        .iter()
        .all(|(key, value)| left.get(key) == Some(value));
    let right_contains_left = left
        .iter()
        .all(|(key, value)| right.get(key) == Some(value));

    match (left_contains_right, right_contains_left) {
        (true, true) => Dominance::Equal,
        (true, false) => Dominance::Dominates,
        (false, true) => Dominance::DominatedBy,
        (false, false) => Dominance::Incomparable,
    }
}

fn predicates(value: &serde_json::Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|text| (key.clone(), text.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The result of comparing two requirements on one key.
///
/// THREE answers, and the third is the point. A bool here collapses "cannot be compared at all"
/// into "compared and came out weaker" -- two facts with opposite remedies, which is the
/// flattening this lane filed as #247, committed here against itself. The two arms that refuse on
/// false happened to do the right thing for both meanings; the arm that used the bool as a
/// TIEBREAKER silently picked a winner when no comparison was possible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Strengthening {
    /// At least as strong, under a registered operator shared by both rules.
    Stronger,
    /// Comparable, and the candidate is weaker.
    Weaker,
    /// No registered operator relates these two. Nothing can be established either way.
    NotComparable,
}

/// Compare `candidate` against `incumbent` under the registered operator.
///
/// The closed set is `minimum`, `maximum`, `set_subset`, `set_superset`, `boolean_required` and
/// `exact`; only `minimum` is implemented in this slice. An UNREGISTERED operator, or two rules
/// naming DIFFERENT operators for one key, yields `NotComparable` -- never a default to permit.
fn compare(candidate: &CodeRuleSpec, incumbent: &CodeRuleSpec) -> Strengthening {
    match (candidate.operator.as_str(), incumbent.operator.as_str()) {
        ("minimum", "minimum") => {
            if candidate.minimum >= incumbent.minimum {
                Strengthening::Stronger
            } else {
                Strengthening::Weaker
            }
        }
        _ => Strengthening::NotComparable,
    }
}

/// Why a resolution produced no contract. Each variant names the source, because a refusal that
/// does not say WHICH input failed sends the caller back to bisecting it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionRefusal {
    /// A declared source could not be read as a code rule.
    SourceUnavailable { artifact: String },
    /// Two requirements on one key are incomparable under the registered operator.
    Conflict {
        conflict_key: String,
        rules: Vec<String>,
    },
    /// The requirements are comparable but nothing orders the RULES.
    PrecedenceUnresolved {
        conflict_key: String,
        rules: Vec<String>,
    },
    /// An owner override is incomplete, or is asserted against a class that cannot be waived.
    WaiverInvalid { rule: String, missing: String },
}

/// What happened to every declared rule.
///
/// Step 8 of the design, and it is not bookkeeping. A contract listing only what it INCLUDED
/// cannot answer the question an owner actually asks -- why did the rule I wrote not apply? -- and
/// a rule absent from the output is otherwise indistinguishable from one never declared.
///
/// Every field is a sorted list for the same reason the contract is: a record whose order depends
/// on input order breaks the byte-identity the acceptance criterion promises.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleRecord {
    /// Contributed a requirement.
    pub included: Vec<String>,
    /// Applied, but outranked on its key by a more specific rule.
    pub shadowed: Vec<String>,
    /// Set aside by a complete owner override.
    pub waived: Vec<String>,
    /// Outside its validity window at the supplied clock.
    pub expired: Vec<String>,
    /// Not of a kind this resolver interprets.
    pub denied: Vec<String>,
}

/// The immutable output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCodeContract {
    /// Strongest requirement per conflict key.
    pub requirements: BTreeMap<String, u32>,
    /// What happened to every declared rule.
    pub record: RuleRecord,
}

impl ResolvedCodeContract {
    /// The contract as canonical bytes -- what a consumer digests and compares.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let value = serde_json::json!({
            "requirements": self.requirements,
            "record": {
                "included": self.record.included,
                "shadowed": self.record.shadowed,
                "waived": self.record.waived,
                "expired": self.record.expired,
                "denied": self.record.denied,
            },
        });
        graphhelm_protocols::canonical_json(&value).into_bytes()
    }
}

/// Accumulate the strongest requirement per key and record what happened to every rule.
pub fn resolve_code_contract(
    sources: &[DevelopmentEnvelope],
    now: DateTime<Utc>,
) -> Result<ResolvedCodeContract, ResolutionRefusal> {
    let mut requirements: BTreeMap<String, u32> = BTreeMap::new();
    // ORDERED, and G7 is the reason. An unordered set here is invisible to every other check: the
    // map above survives one, because `canonical_json` sorts object KEYS and a hash-ordered map
    // still serialises identically. It does NOT sort ARRAYS -- so the only container whose order
    // reaches the emitted bytes is the one feeding a JSON list, and that is this one.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut shadowed: BTreeSet<String> = BTreeSet::new();
    let mut denied: BTreeSet<String> = BTreeSet::new();

    // NOT `filter_map`. The idiomatic version silently drops a source it cannot read and returns
    // a contract that looks complete -- G5 caught exactly that, with two rules declared and one
    // recorded. An unreadable source is a REFUSAL, because nothing downstream can distinguish
    // "these were all the rules" from "these were the rules that happened to parse".
    let mut readable: Vec<(String, CodeRuleSpec)> = Vec::with_capacity(sources.len());
    for source in sources {
        let artifact = source.metadata.id.as_str().to_owned();
        if source.kind != DevelopmentKind::CodeRule {
            denied.insert(artifact);
            continue;
        }
        let spec = serde_json::from_value::<CodeRuleSpec>(source.spec.clone()).map_err(|_| {
            ResolutionRefusal::SourceUnavailable {
                artifact: artifact.clone(),
            }
        })?;
        readable.push((artifact, spec));
    }

    // STEPS 3 and 4, before any comparison. A rule outside its window or set aside by a complete
    // override never reaches the comparison at all -- and each is RECORDED under its own name,
    // because "did not apply" has several causes and the owner's next question is which one.
    let mut expired: BTreeSet<String> = BTreeSet::new();
    let mut waived: BTreeSet<String> = BTreeSet::new();
    let mut live: Vec<(String, CodeRuleSpec)> = Vec::with_capacity(readable.len());

    for (rule_id, spec) in readable {
        if spec.valid_until.is_some_and(|until| now > until) {
            expired.insert(rule_id);
            continue;
        }

        if let Some(waiver) = spec.waiver.as_ref() {
            // CLASS FIRST, and the order is the point. A structural rule is unwaivable regardless
            // of how complete the override is; checking completeness first would let a correctly
            // filled form decide a question that is not about completeness at all.
            if spec.enforcement == "structural" {
                return Err(ResolutionRefusal::WaiverInvalid {
                    rule: rule_id,
                    missing: "<structural rules cannot be waived>".to_owned(),
                });
            }

            if let Some(missing) = REQUIRED_WAIVER_FIELDS
                .iter()
                .find(|field| waiver.get(*field).is_none_or(serde_json::Value::is_null))
            {
                return Err(ResolutionRefusal::WaiverInvalid {
                    rule: rule_id,
                    missing: (*missing).to_owned(),
                });
            }

            waived.insert(rule_id);
            continue;
        }

        live.push((rule_id, spec));
    }

    // Group by conflict key first: comparison is a question about a KEY, never about the input
    // order. Ordered map so the refusal a caller sees does not depend on which key hashed first.
    let mut by_key: BTreeMap<String, Vec<(String, CodeRuleSpec)>> = BTreeMap::new();
    for (rule_id, spec) in live {
        by_key
            .entry(spec.conflict_key.clone())
            .or_default()
            .push((rule_id, spec));
    }

    for (conflict_key, mut group) in by_key {
        // Deterministic within the group too, for the same reason.
        group.sort_by(|left, right| left.0.cmp(&right.0));

        // EVERY PAIR, before the fold. Incomparability is a property of a PAIR and it is NOT
        // transitive: two rules can be incomparable with each other while both are dominated by a
        // third. A fold compares each rule only against the running winner -- n-1 comparisons
        // where the property lives in all n(n-1)/2 pairs -- so whether such a pair ever met
        // depended on which rule the fold happened to be holding, and the order is the sorted rule
        // ID. Renaming a rule, an edit with no semantic content, decided whether step 7 fired.
        for (left_index, (left_id, left)) in group.iter().enumerate() {
            for (right_id, right) in group.iter().skip(left_index + 1) {
                let left_selector =
                    serde_json::to_value(&left.selector).unwrap_or(serde_json::Value::Null);
                let right_selector =
                    serde_json::to_value(&right.selector).unwrap_or(serde_json::Value::Null);
                if dominance(&left_selector, &right_selector) == Dominance::Incomparable {
                    let mut rules = vec![left_id.clone(), right_id.clone()];
                    rules.sort();
                    return Err(ResolutionRefusal::PrecedenceUnresolved {
                        conflict_key,
                        rules,
                    });
                }
            }
        }

        let mut strongest: Option<(String, CodeRuleSpec)> = None;
        for (rule_id, spec) in group {
            let Some((held_id, held)) = strongest.take() else {
                seen.insert(rule_id.clone());
                strongest = Some((rule_id, spec));
                continue;
            };

            let named = || {
                let mut rules = vec![held_id.clone(), rule_id.clone()];
                rules.sort();
                rules
            };

            let winner = match dominance(
                &serde_json::to_value(&spec.selector).unwrap_or(serde_json::Value::Null),
                &serde_json::to_value(&held.selector).unwrap_or(serde_json::Value::Null),
            ) {
                // STEP 7. The values are perfectly comparable; what is missing is an ordering
                // between the RULES. Refusing here is what stops one selector dimension from
                // silently outranking another.
                Dominance::Incomparable => {
                    return Err(ResolutionRefusal::PrecedenceUnresolved {
                        conflict_key,
                        rules: named(),
                    });
                }
                // STEP 5. The more specific rule must not weaken the one it descends from, and
                // must be able to establish that it does not.
                Dominance::Dominates => match compare(&spec, &held) {
                    Strengthening::Stronger => {
                        shadowed.insert(held_id.clone());
                        (rule_id.clone(), spec)
                    }
                    Strengthening::Weaker | Strengthening::NotComparable => {
                        return Err(ResolutionRefusal::Conflict {
                            conflict_key,
                            rules: named(),
                        });
                    }
                },
                Dominance::DominatedBy => match compare(&held, &spec) {
                    Strengthening::Stronger => {
                        shadowed.insert(rule_id.clone());
                        (held_id.clone(), held)
                    }
                    Strengthening::Weaker | Strengthening::NotComparable => {
                        return Err(ResolutionRefusal::Conflict {
                            conflict_key,
                            rules: named(),
                        });
                    }
                },
                // Same population: both apply, and the stronger wins -- but ONLY if a registered
                // operator can say which is stronger. Without one there is no basis to choose, and
                // choosing anyway lets sort order decide, which is the default-permit failure
                // wearing a different hat.
                Dominance::Equal => match compare(&spec, &held) {
                    Strengthening::Stronger => {
                        shadowed.insert(held_id.clone());
                        (rule_id.clone(), spec)
                    }
                    Strengthening::Weaker => {
                        shadowed.insert(rule_id.clone());
                        (held_id.clone(), held)
                    }
                    Strengthening::NotComparable => {
                        return Err(ResolutionRefusal::Conflict {
                            conflict_key,
                            rules: named(),
                        });
                    }
                },
            };

            seen.insert(rule_id);
            strongest = Some(winner);
        }

        if let Some((_, spec)) = strongest {
            requirements.insert(conflict_key, spec.minimum);
        }
    }

    let included: Vec<String> = seen.difference(&shadowed).cloned().collect();

    Ok(ResolvedCodeContract {
        requirements,
        record: RuleRecord {
            included,
            shadowed: shadowed.into_iter().collect(),
            waived: waived.into_iter().collect(),
            expired: expired.into_iter().collect(),
            denied: denied.into_iter().collect(),
        },
    })
}
