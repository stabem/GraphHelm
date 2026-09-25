use std::{fmt, mem::MaybeUninit};

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ValueLimits {
    pub(crate) max_depth: usize,
    pub(crate) max_values: usize,
    pub(crate) max_key_bytes: usize,
    pub(crate) max_string_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ValueMetrics {
    pub(crate) values: usize,
    pub(crate) key_bytes: usize,
    pub(crate) string_bytes: usize,
    pub(crate) max_depth: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ValueBudget {
    metrics: ValueMetrics,
    failure: Option<BoundedValueError>,
}

impl ValueBudget {
    pub(crate) fn metrics(&self) -> ValueMetrics {
        self.metrics
    }

    fn charge_value(&mut self, depth: usize, limits: ValueLimits) -> Result<(), BoundedValueError> {
        if depth > limits.max_depth {
            return self.fail(BoundedValueError::DepthLimit);
        }
        let Some(values) = self.metrics.values.checked_add(1) else {
            return self.fail(BoundedValueError::ValueLimit);
        };
        if values > limits.max_values {
            return self.fail(BoundedValueError::ValueLimit);
        }
        self.metrics.values = values;
        self.metrics.max_depth = self.metrics.max_depth.max(depth);
        Ok(())
    }

    fn charge_key(&mut self, bytes: usize, limits: ValueLimits) -> Result<(), BoundedValueError> {
        let Some(key_bytes) = self.metrics.key_bytes.checked_add(bytes) else {
            return self.fail(BoundedValueError::KeyBytesLimit);
        };
        if key_bytes > limits.max_key_bytes {
            return self.fail(BoundedValueError::KeyBytesLimit);
        }
        self.metrics.key_bytes = key_bytes;
        Ok(())
    }

    fn charge_string(
        &mut self,
        bytes: usize,
        limits: ValueLimits,
    ) -> Result<(), BoundedValueError> {
        let Some(string_bytes) = self.metrics.string_bytes.checked_add(bytes) else {
            return self.fail(BoundedValueError::StringBytesLimit);
        };
        if string_bytes > limits.max_string_bytes {
            return self.fail(BoundedValueError::StringBytesLimit);
        }
        self.metrics.string_bytes = string_bytes;
        Ok(())
    }

    fn fail<T>(&mut self, error: BoundedValueError) -> Result<T, BoundedValueError> {
        self.failure.get_or_insert(error);
        Err(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoundedValueError {
    Invalid,
    InvalidUtf8,
    AnchorOrAlias,
    ByteOrderMark,
    Nul,
    DepthLimit,
    ValueLimit,
    KeyBytesLimit,
    StringBytesLimit,
}

impl fmt::Display for BoundedValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "invalid document",
            Self::InvalidUtf8 => "document is not UTF-8",
            Self::AnchorOrAlias => "YAML anchors and aliases are not allowed",
            Self::ByteOrderMark => "YAML byte-order marks are not allowed after stream start",
            Self::Nul => "YAML NUL characters are not allowed",
            Self::DepthLimit => "document depth limit exceeded",
            Self::ValueLimit => "document value limit exceeded",
            Self::KeyBytesLimit => "document key byte limit exceeded",
            Self::StringBytesLimit => "document string byte limit exceeded",
        })
    }
}

impl std::error::Error for BoundedValueError {}

pub(crate) fn parse_json(
    bytes: &[u8],
    limits: ValueLimits,
    budget: &mut ValueBudget,
) -> Result<Value, BoundedValueError> {
    if let Some(error) = budget.failure {
        return Err(error);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = BoundedValueSeed::root(limits, budget)
        .deserialize(&mut deserializer)
        .map_err(|_| budget.failure.unwrap_or(BoundedValueError::Invalid))?;
    deserializer.end().map_err(|_| BoundedValueError::Invalid)?;
    Ok(value)
}

pub(crate) fn parse_yaml(
    bytes: &[u8],
    limits: ValueLimits,
    budget: &mut ValueBudget,
) -> Result<Value, BoundedValueError> {
    if let Some(error) = budget.failure {
        return Err(error);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| BoundedValueError::InvalidUtf8)?;
    let normalized = text.strip_prefix('\u{feff}').unwrap_or(text);
    if normalized.contains('\u{feff}') {
        return Err(BoundedValueError::ByteOrderMark);
    }
    if normalized.contains('\0') {
        return Err(BoundedValueError::Nul);
    }
    preflight_yaml_events(normalized.as_bytes(), limits)?;

    BoundedValueSeed::root(limits, budget)
        .deserialize(serde_yaml_ng::Deserializer::from_str(normalized))
        .map_err(|_| budget.failure.unwrap_or(BoundedValueError::Invalid))
}

struct BoundedValueSeed<'a> {
    limits: ValueLimits,
    budget: &'a mut ValueBudget,
    depth: usize,
}

impl<'a> BoundedValueSeed<'a> {
    fn root(limits: ValueLimits, budget: &'a mut ValueBudget) -> Self {
        Self {
            limits,
            budget,
            depth: 1,
        }
    }
}

impl<'de> DeserializeSeed<'de> for BoundedValueSeed<'_> {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(BoundedValueVisitor {
            limits: self.limits,
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct BoundedValueVisitor<'a> {
    limits: ValueLimits,
    budget: &'a mut ValueBudget,
    depth: usize,
}

impl BoundedValueVisitor<'_> {
    fn charge_value<E>(&mut self) -> Result<(), E>
    where
        E: de::Error,
    {
        self.budget
            .charge_value(self.depth, self.limits)
            .map_err(E::custom)
    }

    fn charge_string_value<E>(&mut self, bytes: usize) -> Result<(), E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        self.budget
            .charge_string(bytes, self.limits)
            .map_err(E::custom)
    }
}

impl<'de> Visitor<'de> for BoundedValueVisitor<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON-compatible value")
    }

    fn visit_bool<E>(mut self, value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(mut self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        Ok(Value::Number(value.into()))
    }

    fn visit_i128<E>(mut self, value: i128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        Number::from_i128(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("integer is outside the JSON number range"))
    }

    fn visit_u64<E>(mut self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        Ok(Value::Number(value.into()))
    }

    fn visit_u128<E>(mut self, value: u128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        Number::from_u128(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("integer is outside the JSON number range"))
    }

    fn visit_f64<E>(mut self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        Ok(Number::from_f64(value).map_or(Value::Null, Value::Number))
    }

    fn visit_char<E>(mut self, value: char) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_string_value(value.len_utf8())?;
        Ok(Value::String(value.to_string()))
    }

    fn visit_str<E>(mut self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_string_value(value.len())?;
        Ok(Value::String(value.to_owned()))
    }

    fn visit_borrowed_str<E>(mut self, value: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_string_value(value.len())?;
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(mut self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_string_value(value.len())?;
        Ok(Value::String(value))
    }

    fn visit_none<E>(mut self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        Ok(Value::Null)
    }

    fn visit_unit<E>(mut self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_value()?;
        Ok(Value::Null)
    }

    fn visit_seq<A>(mut self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        self.charge_value()?;
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(BoundedValueSeed {
            limits: self.limits,
            budget: &mut *self.budget,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        self.charge_value()?;
        let mut values = Map::new();
        while let Some(key) = map.next_key_seed(BoundedKeySeed {
            limits: self.limits,
            budget: &mut *self.budget,
        })? {
            let value = map.next_value_seed(BoundedValueSeed {
                limits: self.limits,
                budget: &mut *self.budget,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

struct BoundedKeySeed<'a> {
    limits: ValueLimits,
    budget: &'a mut ValueBudget,
}

impl<'de> DeserializeSeed<'de> for BoundedKeySeed<'_> {
    type Value = String;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_string(BoundedKeyVisitor {
            limits: self.limits,
            budget: self.budget,
        })
    }
}

struct BoundedKeyVisitor<'a> {
    limits: ValueLimits,
    budget: &'a mut ValueBudget,
}

impl BoundedKeyVisitor<'_> {
    fn key<E>(self, value: &str) -> Result<String, E>
    where
        E: de::Error,
    {
        self.budget
            .charge_key(value.len(), self.limits)
            .map_err(E::custom)?;
        Ok(value.to_owned())
    }
}

impl<'de> Visitor<'de> for BoundedKeyVisitor<'_> {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded string map key")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.key(value)
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.key(value)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.budget
            .charge_key(value.len(), self.limits)
            .map_err(E::custom)?;
        Ok(value)
    }
}

fn preflight_yaml_events(bytes: &[u8], limits: ValueLimits) -> Result<(), BoundedValueError> {
    use unsafe_libyaml::{
        YAML_ALIAS_EVENT, YAML_MAPPING_END_EVENT, YAML_MAPPING_START_EVENT, YAML_SCALAR_EVENT,
        YAML_SEQUENCE_END_EVENT, YAML_SEQUENCE_START_EVENT, YAML_STREAM_END_EVENT, yaml_event_t,
        yaml_parser_initialize, yaml_parser_parse, yaml_parser_set_input_string, yaml_parser_t,
    };

    // A JSON-compatible document with V values has at most V node-start/scalar events,
    // V container-end events, V-1 map-key events, and four stream/document boundary events.
    // Enforcing this conservative ceiling in libyaml's event stream prevents serde_yaml_ng's
    // document loader from retaining an attacker-controlled number of tiny events before the
    // DeserializeSeed gets a chance to enforce ValueBudget.
    let max_events = limits
        .max_values
        .checked_mul(3)
        .and_then(|value| value.checked_add(4))
        .unwrap_or(usize::MAX);
    let input_len = u64::try_from(bytes.len()).map_err(|_| BoundedValueError::Invalid)?;

    let mut parser_storage = MaybeUninit::<yaml_parser_t>::uninit();
    let parser = parser_storage.as_mut_ptr();

    // SAFETY: parser points to live, correctly aligned MaybeUninit storage. On successful
    // initialization ParserGuard deletes it before that storage leaves scope. The input slice
    // remains borrowed and unchanged for the guard's whole lifetime.
    if unsafe { yaml_parser_initialize(parser) }.fail {
        return Err(BoundedValueError::Invalid);
    }
    let _parser_guard = RawYamlParserGuard(parser);
    // SAFETY: the initialized parser is exclusively owned here, and bytes stays valid until the
    // parser guard is dropped. unsafe-libyaml's size type is u64 on every supported target.
    unsafe { yaml_parser_set_input_string(parser, bytes.as_ptr(), input_len) };

    let mut event_count = 0usize;
    let mut collection_depth = 0usize;
    loop {
        let mut event_storage = MaybeUninit::<yaml_event_t>::uninit();
        let event = event_storage.as_mut_ptr();
        // SAFETY: parser is initialized and exclusively borrowed, and event points to aligned
        // uninitialized storage which yaml_parser_parse initializes on success.
        if unsafe { yaml_parser_parse(parser, event) }.fail {
            return Err(BoundedValueError::Invalid);
        }
        let event_guard = RawYamlEventGuard(event);
        // SAFETY: yaml_parser_parse succeeded, so the complete event is initialized until the
        // event guard deletes it below.
        let event_type = unsafe { (*event).type_ };
        event_count = event_count
            .checked_add(1)
            .ok_or(BoundedValueError::ValueLimit)?;
        if event_count > max_events {
            return Err(BoundedValueError::ValueLimit);
        }

        // SAFETY: each union field is read only for its matching libyaml event discriminant.
        let has_anchor_or_alias = unsafe {
            if event_type == YAML_ALIAS_EVENT {
                true
            } else if event_type == YAML_SCALAR_EVENT {
                !(*event).data.scalar.anchor.is_null()
            } else if event_type == YAML_SEQUENCE_START_EVENT {
                !(*event).data.sequence_start.anchor.is_null()
            } else if event_type == YAML_MAPPING_START_EVENT {
                !(*event).data.mapping_start.anchor.is_null()
            } else {
                false
            }
        };
        if has_anchor_or_alias {
            return Err(BoundedValueError::AnchorOrAlias);
        }

        let starts_node = event_type == YAML_SCALAR_EVENT
            || event_type == YAML_SEQUENCE_START_EVENT
            || event_type == YAML_MAPPING_START_EVENT;
        if starts_node {
            let node_depth = collection_depth
                .checked_add(1)
                .ok_or(BoundedValueError::DepthLimit)?;
            if node_depth > limits.max_depth {
                return Err(BoundedValueError::DepthLimit);
            }
        }
        if event_type == YAML_SEQUENCE_START_EVENT || event_type == YAML_MAPPING_START_EVENT {
            collection_depth = collection_depth
                .checked_add(1)
                .ok_or(BoundedValueError::DepthLimit)?;
        } else if event_type == YAML_SEQUENCE_END_EVENT || event_type == YAML_MAPPING_END_EVENT {
            collection_depth = collection_depth
                .checked_sub(1)
                .ok_or(BoundedValueError::Invalid)?;
        }

        let stream_ended = event_type == YAML_STREAM_END_EVENT;
        drop(event_guard);
        if stream_ended {
            return Ok(());
        }
    }
}

struct RawYamlParserGuard(*mut unsafe_libyaml::yaml_parser_t);

impl Drop for RawYamlParserGuard {
    fn drop(&mut self) {
        // SAFETY: this guard is created only after successful parser initialization and is the
        // unique owner responsible for exactly one deletion.
        unsafe { unsafe_libyaml::yaml_parser_delete(self.0) };
    }
}

struct RawYamlEventGuard(*mut unsafe_libyaml::yaml_event_t);

impl Drop for RawYamlEventGuard {
    fn drop(&mut self) {
        // SAFETY: this guard is created only after yaml_parser_parse initialized the event and is
        // the unique owner responsible for exactly one deletion.
        unsafe { unsafe_libyaml::yaml_event_delete(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> ValueLimits {
        ValueLimits {
            max_depth: 8,
            max_values: 8,
            max_key_bytes: 16,
            max_string_bytes: 16,
        }
    }

    #[test]
    fn json_value_limit_stops_before_the_excess_value_is_built() {
        let mut limits = limits();
        limits.max_values = 2;
        let mut budget = ValueBudget::default();

        assert_eq!(
            parse_json(b"[null,null]", limits, &mut budget),
            Err(BoundedValueError::ValueLimit)
        );
        assert_eq!(budget.metrics().values, 2);
    }

    #[test]
    fn json_depth_limit_stops_before_the_nested_container_is_built() {
        let mut limits = limits();
        limits.max_depth = 1;
        let mut budget = ValueBudget::default();

        assert_eq!(
            parse_json(b"[[]]", limits, &mut budget),
            Err(BoundedValueError::DepthLimit)
        );
        assert_eq!(budget.metrics().max_depth, 1);
    }

    #[test]
    fn json_key_and_string_budgets_are_checked_before_insertion() {
        let mut key_limits = limits();
        key_limits.max_key_bytes = 3;
        let mut key_budget = ValueBudget::default();
        assert_eq!(
            parse_json(b"{\"long\":null}", key_limits, &mut key_budget),
            Err(BoundedValueError::KeyBytesLimit)
        );
        assert_eq!(key_budget.metrics().key_bytes, 0);

        let mut string_limits = limits();
        string_limits.max_string_bytes = 3;
        let mut string_budget = ValueBudget::default();
        assert_eq!(
            parse_json(b"\"long\"", string_limits, &mut string_budget),
            Err(BoundedValueError::StringBytesLimit)
        );
        assert_eq!(string_budget.metrics().string_bytes, 0);
    }

    #[test]
    fn json_requires_one_complete_document() {
        let mut budget = ValueBudget::default();
        assert_eq!(
            parse_json(b"{\"ok\":true} trailing", limits(), &mut budget),
            Err(BoundedValueError::Invalid)
        );
    }

    #[test]
    fn yaml_normalizes_one_leading_utf8_bom() {
        let mut budget = ValueBudget::default();
        let value = parse_yaml(b"\xEF\xBB\xBFvalue: accepted\n", limits(), &mut budget).unwrap();

        assert_eq!(value["value"], "accepted");
    }

    #[test]
    fn yaml_rejects_utf16_embedded_bom_nul_and_aliases_before_values() {
        for (source, expected) in [
            (&b"\xFF\xFEv\0"[..], BoundedValueError::InvalidUtf8),
            (&b"\xFE\xFF\0v"[..], BoundedValueError::InvalidUtf8),
            (
                &b"value: \xEF\xBB\xBFhidden\n"[..],
                BoundedValueError::ByteOrderMark,
            ),
            (&b"value: hidden\0\n"[..], BoundedValueError::Nul),
            (
                &b"shared: &shared value\nalias: *shared\n"[..],
                BoundedValueError::AnchorOrAlias,
            ),
        ] {
            let mut budget = ValueBudget::default();
            assert_eq!(parse_yaml(source, limits(), &mut budget), Err(expected));
            assert_eq!(budget.metrics().values, 0);
        }
    }

    #[test]
    fn yaml_comment_line_breaks_cannot_hide_anchors_or_aliases() {
        for line_break in ['\u{0085}', '\u{2028}', '\u{2029}'] {
            for source in [
                format!("# comment{line_break}value: &blocked scalar\n"),
                format!("# comment{line_break}value: *blocked\n"),
            ] {
                let mut budget = ValueBudget::default();

                assert_eq!(
                    parse_yaml(source.as_bytes(), limits(), &mut budget),
                    Err(BoundedValueError::AnchorOrAlias),
                    "line break U+{:04X} must terminate a YAML comment",
                    u32::from(line_break)
                );
                assert_eq!(budget.metrics().values, 0);
            }
        }
    }

    #[test]
    fn yaml_scalar_text_that_looks_like_an_anchor_or_alias_is_allowed() {
        for source in [
            "value: plain text &word and *word\n",
            "value: |\n  literal &word\n  alias-like *word\n",
            "value: >\n  folded &word\n  alias-like *word\n",
        ] {
            let mut scalar_limits = limits();
            scalar_limits.max_string_bytes = 128;
            let mut budget = ValueBudget::default();
            let parsed = parse_yaml(source.as_bytes(), scalar_limits, &mut budget).unwrap();

            assert!(parsed["value"].as_str().unwrap().contains("&word"));
            assert!(parsed["value"].as_str().unwrap().contains("*word"));
        }
    }

    #[test]
    fn yaml_tiny_node_flood_is_rejected_before_value_deserialization() {
        let mut source = String::new();
        for _ in 0..10_000 {
            source.push_str("- x\n");
        }
        assert!(source.len() < 4 * 1024 * 1024);

        let mut constrained = limits();
        constrained.max_values = 2;
        let mut budget = ValueBudget::default();

        assert_eq!(
            parse_yaml(source.as_bytes(), constrained, &mut budget),
            Err(BoundedValueError::ValueLimit)
        );
        assert_eq!(
            budget.metrics().values,
            0,
            "the streaming preflight must reject before serde_yaml_ng buffers and deserializes the document"
        );
    }

    #[test]
    fn yaml_depth_flood_is_rejected_before_value_deserialization() {
        let source = format!("{}null{}", "[".repeat(64), "]".repeat(64));
        let mut constrained = limits();
        constrained.max_depth = 8;
        constrained.max_values = 128;
        let mut budget = ValueBudget::default();

        assert_eq!(
            parse_yaml(source.as_bytes(), constrained, &mut budget),
            Err(BoundedValueError::DepthLimit)
        );
        assert_eq!(
            budget.metrics().values,
            0,
            "the streaming preflight must reject excessive nesting before serde_yaml_ng buffers and deserializes the document"
        );
    }

    #[test]
    fn yaml_uses_the_same_value_and_text_limits() {
        let mut value_limits = limits();
        value_limits.max_values = 2;
        let mut value_budget = ValueBudget::default();
        assert_eq!(
            parse_yaml(b"- null\n- null\n", value_limits, &mut value_budget),
            Err(BoundedValueError::ValueLimit)
        );

        let mut text_limits = limits();
        text_limits.max_string_bytes = 3;
        let mut text_budget = ValueBudget::default();
        assert_eq!(
            parse_yaml(b"long\n", text_limits, &mut text_budget),
            Err(BoundedValueError::StringBytesLimit)
        );
    }

    #[test]
    fn normal_json_and_yaml_values_parse_with_metrics() {
        let mut json_budget = ValueBudget::default();
        let json = parse_json(
            br#"{"name":"graphhelm","enabled":true}"#,
            limits(),
            &mut json_budget,
        )
        .unwrap();
        assert_eq!(json["name"], "graphhelm");
        assert_eq!(json_budget.metrics().values, 3);

        let mut yaml_budget = ValueBudget::default();
        let yaml = parse_yaml(
            b"name: graphhelm\nenabled: true\n",
            limits(),
            &mut yaml_budget,
        )
        .unwrap();
        assert_eq!(yaml, json);
        assert_eq!(yaml_budget.metrics().values, 3);
    }
}
