use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::Diagnostic;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

use crate::{
    MAX_FILE_BYTES, MAX_JSON_DEPTH, MAX_MIGRATION_OPERATIONS, MAX_POINTER_BYTES, SchemaCatalog,
    SchemaDigest,
};

const MIGRATION_UNSUPPORTED: &str = "GHM001_MIGRATION_UNSUPPORTED";
const SCHEMA_HASH_MISMATCH: &str = "GHM002_SCHEMA_HASH_MISMATCH";
const PATCH_INVALID: &str = "GHM003_PATCH_INVALID";
const DESTINATION_INVALID: &str = "GHM004_DESTINATION_INVALID";
const DIAGNOSTIC_SOURCE: &str = "migration";

/// One strict, declarative JSON Patch migration between exact schema versions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationManifest {
    pub format_version: u32,
    pub schema: String,
    pub from_version: Version,
    pub to_version: Version,
    pub source_schema_hash: SchemaDigest,
    pub target_schema_hash: SchemaDigest,
    pub operations: Vec<PatchOperation>,
}

/// The supported, non-executable RFC 6902 operation subset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum PatchOperation {
    Add { path: String, value: Value },
    Remove { path: String },
    Replace { path: String, value: Value },
    Move { from: String, path: String },
    Copy { from: String, path: String },
    Test { path: String, value: Value },
}

/// Exact source and target catalog metadata supplied by the I/O-owning caller.
#[derive(Clone, Copy, Debug)]
pub struct MigrationCatalogs<'a> {
    pub source: &'a SchemaCatalog,
    pub target: &'a SchemaCatalog,
}

/// Transactional migration result. Failed applications never contain a document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub ok: bool,
    pub document: Option<Value>,
    pub diagnostics: Vec<Diagnostic>,
}

impl MigrationResult {
    fn success(document: Value) -> Self {
        Self {
            ok: true,
            document: Some(document),
            diagnostics: Vec::new(),
        }
    }

    fn failure(diagnostic: Diagnostic) -> Self {
        Self {
            ok: false,
            document: None,
            diagnostics: vec![diagnostic],
        }
    }
}

/// Applies a validated manifest to an isolated clone and publishes only a fully valid result.
#[must_use]
pub fn apply_migration<Source, Target>(
    document: &Value,
    manifest: &MigrationManifest,
    catalogs: &MigrationCatalogs<'_>,
    validate_source: Source,
    validate_target: Target,
) -> MigrationResult
where
    Source: Fn(&Value) -> Vec<Diagnostic>,
    Target: Fn(&Value) -> Vec<Diagnostic>,
{
    if let Err(diagnostic) =
        validate_migration_manifest(manifest, Some(catalogs.source), catalogs.target)
    {
        return MigrationResult::failure(diagnostic);
    }
    if let Err(diagnostic) = validate_document_bound(document, "/source") {
        return MigrationResult::failure(diagnostic);
    }
    if !validate_source(document).is_empty() {
        return MigrationResult::failure(diagnostic(
            DESTINATION_INVALID,
            "source document does not satisfy its schema",
            "/source",
        ));
    }

    let mut candidate = document.clone();
    for (index, operation) in manifest.operations.iter().enumerate() {
        if let Err(field) = apply_operation(&mut candidate, operation) {
            return MigrationResult::failure(diagnostic(
                PATCH_INVALID,
                "JSON Patch operation is invalid",
                &format!("/operations/{index}/{field}"),
            ));
        }
        if is_structural(operation) {
            let path = format!("/operations/{index}/path");
            if let Err(diagnostic) = validate_document_bound(&candidate, &path) {
                return MigrationResult::failure(diagnostic);
            }
        }
    }

    if !validate_target(&candidate).is_empty() {
        return MigrationResult::failure(diagnostic(
            DESTINATION_INVALID,
            "destination document does not satisfy its schema",
            "/destination",
        ));
    }

    MigrationResult::success(candidate)
}

/// Selects the only explicit, contiguous chain between exact versions.
pub fn plan_migration_chain(
    schema: &str,
    from: &Version,
    to: &Version,
    manifests: &[MigrationManifest],
) -> Result<Vec<MigrationManifest>, Vec<Diagnostic>> {
    if from > to {
        return Err(vec![unsupported(
            "migration downgrades are not supported",
            "/toVersion",
        )]);
    }
    if from == to {
        return Ok(Vec::new());
    }

    let mut outgoing = BTreeMap::<Version, (usize, &MigrationManifest)>::new();
    for (index, manifest) in manifests.iter().enumerate() {
        if manifest.schema != schema {
            continue;
        }
        if manifest.format_version != 1 {
            return Err(vec![unsupported(
                "migration format version is unsupported",
                &format!("/manifests/{index}/formatVersion"),
            )]);
        }
        if !stable(&manifest.from_version)
            || !stable(&manifest.to_version)
            || manifest.from_version >= manifest.to_version
        {
            return Err(vec![unsupported(
                "migration edge must be a stable, strictly increasing version transition",
                &format!("/manifests/{index}/toVersion"),
            )]);
        }
        if outgoing
            .insert(manifest.from_version.clone(), (index, manifest))
            .is_some()
        {
            return Err(vec![unsupported(
                "multiple outgoing migrations are not supported",
                &format!("/manifests/{index}/fromVersion"),
            )]);
        }
    }

    let mut current = from.clone();
    let mut visited = BTreeSet::new();
    let mut chain: Vec<MigrationManifest> = Vec::new();
    while current != *to {
        if !visited.insert(current.clone()) {
            return Err(vec![unsupported(
                "migration chain contains a cycle",
                "/fromVersion",
            )]);
        }
        let Some((index, migration)) = outgoing.get(&current).copied() else {
            return Err(vec![unsupported(
                "migration chain contains a gap",
                "/fromVersion",
            )]);
        };
        if migration.to_version > *to {
            return Err(vec![unsupported(
                "migration chain overshoots the requested target",
                &format!("/manifests/{index}/toVersion"),
            )]);
        }
        if chain
            .last()
            .is_some_and(|previous| previous.target_schema_hash != migration.source_schema_hash)
        {
            return Err(vec![unsupported(
                "adjacent migration schema digests do not match",
                &format!("/chain/{}/sourceSchemaHash", chain.len()),
            )]);
        }
        chain.push(migration.clone());
        current = migration.to_version.clone();
    }

    Ok(chain)
}

/// Validates the strict manifest contract against the exact target catalog and, when supplied,
/// the exact source catalog. Passing no source supports target-first filesystem preflight without
/// weakening full validation before evidence credit or migration application.
pub fn validate_migration_manifest(
    manifest: &MigrationManifest,
    source_catalog: Option<&SchemaCatalog>,
    target_catalog: &SchemaCatalog,
) -> Result<(), Diagnostic> {
    if manifest.format_version != 1 {
        return Err(unsupported(
            "migration format version is unsupported",
            "/formatVersion",
        ));
    }
    if manifest.schema.is_empty() {
        return Err(unsupported("migration schema is unavailable", "/schema"));
    }
    if !stable(&manifest.from_version)
        || !stable(&manifest.to_version)
        || manifest.from_version >= manifest.to_version
    {
        return Err(unsupported(
            "migration versions must be stable and strictly increasing",
            "/toVersion",
        ));
    }

    if let Some(source_catalog) = source_catalog {
        let Some(source) = source_catalog.schemas.get(&manifest.schema) else {
            return Err(unsupported("source schema is unavailable", "/schema"));
        };
        if source.document_version != manifest.from_version {
            return Err(unsupported(
                "source schema version does not match the migration",
                "/fromVersion",
            ));
        }
        if source.sha256 != manifest.source_schema_hash {
            return Err(diagnostic(
                SCHEMA_HASH_MISMATCH,
                "source schema digest does not match the catalog",
                "/sourceSchemaHash",
            ));
        }
    }
    let Some(target) = target_catalog.schemas.get(&manifest.schema) else {
        return Err(unsupported("target schema is unavailable", "/schema"));
    };
    if target.document_version != manifest.to_version {
        return Err(unsupported(
            "target schema version does not match the migration",
            "/toVersion",
        ));
    }
    if target.sha256 != manifest.target_schema_hash {
        return Err(diagnostic(
            SCHEMA_HASH_MISMATCH,
            "target schema digest does not match the catalog",
            "/targetSchemaHash",
        ));
    }
    if manifest.operations.len() > MAX_MIGRATION_OPERATIONS {
        return Err(diagnostic(
            PATCH_INVALID,
            "migration exceeds the operation-count limit",
            "/operations",
        ));
    }

    for (index, operation) in manifest.operations.iter().enumerate() {
        validate_operation(index, operation)?;
    }
    Ok(())
}

fn validate_operation(index: usize, operation: &PatchOperation) -> Result<(), Diagnostic> {
    let (path, path_allows_append) = match operation {
        PatchOperation::Add { path, .. } => (path, true),
        PatchOperation::Remove { path }
        | PatchOperation::Replace { path, .. }
        | PatchOperation::Test { path, .. } => (path, false),
        PatchOperation::Move { path, .. } | PatchOperation::Copy { path, .. } => (path, true),
    };
    validate_pointer(index, "path", path, path_allows_append)?;

    if let PatchOperation::Move { from, .. } | PatchOperation::Copy { from, .. } = operation {
        validate_pointer(index, "from", from, false)?;
    }
    if let PatchOperation::Add { value, .. }
    | PatchOperation::Replace { value, .. }
    | PatchOperation::Test { value, .. } = operation
    {
        if exceeds_depth(value) || serialized_size(value).is_none_or(|size| size > MAX_FILE_BYTES) {
            return Err(diagnostic(
                PATCH_INVALID,
                "patch value exceeds a fixed resource limit",
                &format!("/operations/{index}/value"),
            ));
        }
        if contains_remote_reference(value) {
            return Err(diagnostic(
                PATCH_INVALID,
                "patch values may not contain remote references",
                &format!("/operations/{index}/value"),
            ));
        }
    }
    Ok(())
}

fn validate_pointer(
    index: usize,
    field: &str,
    pointer: &str,
    allow_final_append: bool,
) -> Result<(), Diagnostic> {
    if pointer.len() > MAX_POINTER_BYTES {
        return Err(diagnostic(
            PATCH_INVALID,
            "JSON Pointer exceeds the byte limit",
            &format!("/operations/{index}/{field}"),
        ));
    }
    let tokens = parse_pointer(pointer).map_err(|()| {
        diagnostic(
            PATCH_INVALID,
            "JSON Pointer syntax is invalid",
            &format!("/operations/{index}/{field}"),
        )
    })?;
    if tokens.iter().enumerate().any(|(token_index, token)| {
        token == "-" && (!allow_final_append || token_index + 1 != tokens.len())
    }) {
        return Err(diagnostic(
            PATCH_INVALID,
            "array append token is invalid for this pointer",
            &format!("/operations/{index}/{field}"),
        ));
    }
    Ok(())
}

fn apply_operation(document: &mut Value, operation: &PatchOperation) -> Result<(), &'static str> {
    match operation {
        PatchOperation::Add { path, value } => add(
            document,
            &parse_pointer(path).map_err(|()| "path")?,
            value.clone(),
        ),
        PatchOperation::Remove { path } => {
            remove(document, &parse_pointer(path).map_err(|()| "path")?).map(drop)
        }
        PatchOperation::Replace { path, value } => replace(
            document,
            &parse_pointer(path).map_err(|()| "path")?,
            value.clone(),
        ),
        PatchOperation::Move { from, path } => {
            let from_tokens = parse_pointer(from).map_err(|()| "from")?;
            let path_tokens = parse_pointer(path).map_err(|()| "path")?;
            if from_tokens == path_tokens {
                get(document, &from_tokens)?;
                return Ok(());
            }
            if path_tokens.starts_with(&from_tokens) && path_tokens.len() > from_tokens.len() {
                return Err("path");
            }
            let value = remove(document, &from_tokens).map_err(|_| "from")?;
            add(document, &path_tokens, value)
        }
        PatchOperation::Copy { from, path } => {
            let value = get(document, &parse_pointer(from).map_err(|()| "from")?)
                .map_err(|_| "from")?
                .clone();
            add(document, &parse_pointer(path).map_err(|()| "path")?, value)
        }
        PatchOperation::Test { path, value } => {
            if json_test_equal(
                get(document, &parse_pointer(path).map_err(|()| "path")?)?,
                value,
            ) {
                Ok(())
            } else {
                Err("path")
            }
        }
    }
}

fn json_test_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Number(left), Value::Number(right)) => json_numbers_equal(left, right),
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_test_equal(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| json_test_equal(left, right))
                })
        }
        _ => false,
    }
}

fn json_numbers_equal(left: &Number, right: &Number) -> bool {
    if left.is_f64() {
        let Some(left) = left.as_f64() else {
            return false;
        };
        return if right.is_f64() {
            right.as_f64().is_some_and(|right| left == right)
        } else {
            float_equals_integer(left, right)
        };
    }
    if right.is_f64() {
        return right
            .as_f64()
            .is_some_and(|right| float_equals_integer(right, left));
    }
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return left == right;
    }
    matches!((left.as_u64(), right.as_u64()), (Some(left), Some(right)) if left == right)
}

fn float_equals_integer(float: f64, integer: &Number) -> bool {
    const I64_MIN_AS_F64: f64 = -9_223_372_036_854_775_808.0;
    const I64_MAX_EXCLUSIVE_AS_F64: f64 = 9_223_372_036_854_775_808.0;
    const U64_MAX_EXCLUSIVE_AS_F64: f64 = 18_446_744_073_709_551_616.0;

    if !float.is_finite() || float.fract() != 0.0 {
        return false;
    }
    if let Some(integer) = integer.as_i64() {
        return (I64_MIN_AS_F64..I64_MAX_EXCLUSIVE_AS_F64).contains(&float)
            && float as i64 == integer
            && integer as f64 == float;
    }
    if let Some(integer) = integer.as_u64() {
        return (0.0..U64_MAX_EXCLUSIVE_AS_F64).contains(&float)
            && float as u64 == integer
            && integer as f64 == float;
    }
    false
}

fn add(document: &mut Value, tokens: &[String], value: Value) -> Result<(), &'static str> {
    let Some((last, parents)) = tokens.split_last() else {
        *document = value;
        return Ok(());
    };
    match get_mut(document, parents)? {
        Value::Object(map) => {
            map.insert(last.clone(), value);
            Ok(())
        }
        Value::Array(array) => {
            if last == "-" {
                array.push(value);
                return Ok(());
            }
            let index = array_index(last, true, array.len())?;
            array.insert(index, value);
            Ok(())
        }
        _ => Err("path"),
    }
}

fn remove(document: &mut Value, tokens: &[String]) -> Result<Value, &'static str> {
    let Some((last, parents)) = tokens.split_last() else {
        return Err("path");
    };
    match get_mut(document, parents)? {
        Value::Object(map) => map.remove(last).ok_or("path"),
        Value::Array(array) => {
            let index = array_index(last, false, array.len())?;
            Ok(array.remove(index))
        }
        _ => Err("path"),
    }
}

fn replace(document: &mut Value, tokens: &[String], value: Value) -> Result<(), &'static str> {
    if tokens.is_empty() {
        *document = value;
        return Ok(());
    }
    *get_mut(document, tokens)? = value;
    Ok(())
}

fn get<'a>(document: &'a Value, tokens: &[String]) -> Result<&'a Value, &'static str> {
    let mut current = document;
    for token in tokens {
        current = match current {
            Value::Object(map) => map.get(token).ok_or("path")?,
            Value::Array(array) => {
                let index = array_index(token, false, array.len())?;
                &array[index]
            }
            _ => return Err("path"),
        };
    }
    Ok(current)
}

fn get_mut<'a>(document: &'a mut Value, tokens: &[String]) -> Result<&'a mut Value, &'static str> {
    let mut current = document;
    for token in tokens {
        current = match current {
            Value::Object(map) => map.get_mut(token).ok_or("path")?,
            Value::Array(array) => {
                let index = array_index(token, false, array.len())?;
                &mut array[index]
            }
            _ => return Err("path"),
        };
    }
    Ok(current)
}

fn array_index(token: &str, allow_end: bool, len: usize) -> Result<usize, &'static str> {
    if token.is_empty() || (token.len() > 1 && token.starts_with('0')) {
        return Err("path");
    }
    if !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("path");
    }
    let index = token.parse::<usize>().map_err(|_| "path")?;
    if index < len || (allow_end && index == len) {
        Ok(index)
    } else {
        Err("path")
    }
}

fn parse_pointer(pointer: &str) -> Result<Vec<String>, ()> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    let Some(encoded) = pointer.strip_prefix('/') else {
        return Err(());
    };
    encoded.split('/').map(unescape_token).collect()
}

fn unescape_token(token: &str) -> Result<String, ()> {
    let mut decoded = String::with_capacity(token.len());
    let mut characters = token.chars();
    while let Some(character) = characters.next() {
        if character != '~' {
            decoded.push(character);
            continue;
        }
        match characters.next() {
            Some('0') => decoded.push('~'),
            Some('1') => decoded.push('/'),
            _ => return Err(()),
        }
    }
    Ok(decoded)
}

fn validate_document_bound(document: &Value, path: &str) -> Result<(), Diagnostic> {
    if exceeds_depth(document) {
        return Err(diagnostic(
            PATCH_INVALID,
            "document exceeds the nesting-depth limit",
            path,
        ));
    }
    if serialized_size(document).is_none_or(|size| size > MAX_FILE_BYTES) {
        return Err(diagnostic(
            PATCH_INVALID,
            "document exceeds the serialized-size limit",
            path,
        ));
    }
    Ok(())
}

fn exceeds_depth(value: &Value) -> bool {
    let mut pending = vec![(value, 0_usize)];
    while let Some((current, depth)) = pending.pop() {
        if depth > MAX_JSON_DEPTH {
            return true;
        }
        match current {
            Value::Array(values) => {
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                pending.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    false
}

fn serialized_size(value: &Value) -> Option<usize> {
    serde_json::to_vec(value).ok().map(|bytes| bytes.len())
}

fn contains_remote_reference(value: &Value) -> bool {
    let mut pending = vec![value];
    while let Some(current) = pending.pop() {
        match current {
            Value::Array(values) => pending.extend(values),
            Value::Object(values) => {
                if values
                    .get("$ref")
                    .and_then(Value::as_str)
                    .is_some_and(is_remote_reference)
                {
                    return true;
                }
                pending.extend(values.values());
            }
            _ => {}
        }
    }
    false
}

fn is_remote_reference(reference: &str) -> bool {
    if reference.starts_with("//") {
        return true;
    }
    let mut characters = reference.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && characters
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.' | ':')
            })
            .any(|character| character == ':')
}

fn is_structural(operation: &PatchOperation) -> bool {
    !matches!(operation, PatchOperation::Test { .. })
}

fn stable(version: &Version) -> bool {
    version.pre.is_empty() && version.build.is_empty()
}

fn unsupported(message: &str, path: &str) -> Diagnostic {
    diagnostic(MIGRATION_UNSUPPORTED, message, path)
}

fn diagnostic(code: &str, message: &str, path: &str) -> Diagnostic {
    Diagnostic::error(code, message, path, DIAGNOSTIC_SOURCE)
}
