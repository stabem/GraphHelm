use std::collections::BTreeSet;

use serde_json::Value;

use crate::{CatalogResources, MAX_JSON_DEPTH, MAX_POINTER_BYTES};

pub(crate) fn resolved_schema_references(
    resources: &CatalogResources,
    owner: &str,
    schema: &Value,
) -> Result<BTreeSet<String>, BTreeSet<String>> {
    let mut references = BTreeSet::new();
    let mut invalid = BTreeSet::new();
    collect_schema_references(
        resources,
        owner,
        schema,
        "",
        0,
        &mut references,
        &mut invalid,
    );
    if invalid.is_empty() {
        Ok(references)
    } else {
        Err(invalid)
    }
}

fn collect_schema_references(
    resources: &CatalogResources,
    owner: &str,
    schema: &Value,
    pointer: &str,
    depth: usize,
    references: &mut BTreeSet<String>,
    invalid: &mut BTreeSet<String>,
) {
    if depth > MAX_JSON_DEPTH {
        invalid.insert(display_pointer(pointer).to_owned());
        return;
    }
    let Some(keywords) = schema.as_object() else {
        return;
    };

    if let Some(reference) = keywords.get("$ref") {
        match reference
            .as_str()
            .ok_or(())
            .and_then(|reference| resolve_reference(resources, owner, reference))
        {
            Ok(resolved) => {
                references.insert(resolved);
            }
            Err(()) => {
                invalid.insert(join_pointer(pointer, "$ref"));
            }
        }
    }

    for keyword in [
        "properties",
        "patternProperties",
        "$defs",
        "definitions",
        "dependentSchemas",
    ] {
        let Some(children) = keywords.get(keyword).and_then(Value::as_object) else {
            continue;
        };
        let keyword_pointer = join_pointer(pointer, keyword);
        for (name, child) in children {
            collect_schema_references(
                resources,
                owner,
                child,
                &join_pointer(&keyword_pointer, name),
                depth + 1,
                references,
                invalid,
            );
        }
    }

    for keyword in [
        "additionalProperties",
        "unevaluatedProperties",
        "unevaluatedItems",
        "propertyNames",
        "items",
        "contains",
        "contentSchema",
        "not",
        "if",
        "then",
        "else",
    ] {
        let Some(child) = keywords.get(keyword) else {
            continue;
        };
        collect_schema_references(
            resources,
            owner,
            child,
            &join_pointer(pointer, keyword),
            depth + 1,
            references,
            invalid,
        );
    }

    for keyword in ["prefixItems", "allOf", "anyOf", "oneOf"] {
        let Some(children) = keywords.get(keyword).and_then(Value::as_array) else {
            continue;
        };
        let keyword_pointer = join_pointer(pointer, keyword);
        for (index, child) in children.iter().enumerate() {
            collect_schema_references(
                resources,
                owner,
                child,
                &join_pointer(&keyword_pointer, &index.to_string()),
                depth + 1,
                references,
                invalid,
            );
        }
    }
}

pub(crate) fn resolve_reference(
    resources: &CatalogResources,
    owner: &str,
    reference: &str,
) -> Result<String, ()> {
    let start = absolute_reference(resources, owner, reference)?;
    let mut current = start.clone();
    let mut visited = BTreeSet::new();
    loop {
        if visited.len() >= MAX_JSON_DEPTH {
            return Err(());
        }
        if !visited.insert(current.clone()) {
            return Ok(start);
        }
        let (target_owner, target) = referenced_value(resources, &current)?;
        let Some(next) = target.get("$ref").and_then(Value::as_str) else {
            return Ok(start);
        };
        current = absolute_reference(resources, target_owner, next)?;
    }
}

fn absolute_reference(
    resources: &CatalogResources,
    owner: &str,
    reference: &str,
) -> Result<String, ()> {
    let owner_id = resources.catalog.schemas.get(owner).ok_or(())?.id.as_str();
    let (reference_base, fragment) = reference.split_once('#').unwrap_or((reference, ""));
    if reference.matches('#').count() > 1 {
        return Err(());
    }
    let base = if reference_base.is_empty() {
        owner_id.to_owned()
    } else if reference_base.contains("://") {
        reference_base.to_owned()
    } else {
        if reference_base.contains(':') || reference_base.starts_with('/') {
            return Err(());
        }
        let owner_directory = owner_id.rsplit_once('/').ok_or(())?.0;
        normalize_url_path(owner_directory, reference_base)?
    };
    if !resources
        .catalog
        .schemas
        .values()
        .any(|entry| entry.id == base)
    {
        return Err(());
    }
    let fragment = decode_pointer_fragment(fragment)?;
    if fragment.is_empty() {
        Ok(base)
    } else {
        Ok(format!("{base}#{fragment}"))
    }
}

fn decode_pointer_fragment(fragment: &str) -> Result<String, ()> {
    if fragment.len() > MAX_POINTER_BYTES {
        return Err(());
    }
    let bytes = fragment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let high = bytes
            .get(index + 1)
            .copied()
            .and_then(hex_value)
            .ok_or(())?;
        let low = bytes
            .get(index + 2)
            .copied()
            .and_then(hex_value)
            .ok_or(())?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    if decoded.len() > MAX_POINTER_BYTES {
        return Err(());
    }
    let decoded = String::from_utf8(decoded).map_err(|_| ())?;
    if decoded.is_empty() {
        return Ok(decoded);
    }
    if !decoded.starts_with('/') {
        return Err(());
    }
    for token in decoded.split('/').skip(1) {
        validate_pointer_token(token)?;
    }
    Ok(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validate_pointer_token(token: &str) -> Result<(), ()> {
    let bytes = token.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'~' {
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            Some(b'0' | b'1') => index += 2,
            _ => return Err(()),
        }
    }
    Ok(())
}

fn normalize_url_path(directory: &str, relative: &str) -> Result<String, ()> {
    let (origin, path) = directory.split_once("://").ok_or(())?;
    let mut segments = path.split('/').map(str::to_owned).collect::<Vec<_>>();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.len() <= 1 {
                    return Err(());
                }
                segments.pop();
            }
            value => segments.push(value.to_owned()),
        }
    }
    Ok(format!("{origin}://{}", segments.join("/")))
}

/// Resolves an already-ABSOLUTE reference (as `resolve_reference` returns) to its owning schema
/// name and the `Value` it points at. `pub(crate)` because the discriminator-disjointness proof in
/// `compatibility.rs` needs the actual target document, not just its resolved reference string.
pub(crate) fn referenced_value<'a>(
    resources: &'a CatalogResources,
    absolute: &str,
) -> Result<(&'a str, &'a Value), ()> {
    let (base, fragment) = absolute.split_once('#').unwrap_or((absolute, ""));
    let (owner, _) = resources
        .catalog
        .schemas
        .iter()
        .find(|(_, entry)| entry.id == base)
        .ok_or(())?;
    let root = resources.schemas.get(owner).ok_or(())?;
    let target = if fragment.is_empty() {
        root
    } else {
        root.pointer(fragment).ok_or(())?
    };
    Ok((owner, target))
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
