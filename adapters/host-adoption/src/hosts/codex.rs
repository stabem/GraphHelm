//! Documented config.toml surfaces only; plugin browser state is deliberately not a registry API.
use graphhelm_protocols::adoption::AdoptionError;

pub fn disable_skills(
    before: &str,
    accepted: &[String],
    supported: bool,
) -> Result<String, AdoptionError> {
    if !supported {
        return Err(super::invalid());
    }
    let mut value: toml::Value = toml::from_str(before).map_err(|_| super::invalid())?;
    let table = value.as_table_mut().ok_or_else(super::invalid)?;
    let skills = table
        .entry("skills")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(super::invalid)?;
    let config = skills
        .entry("config")
        .or_insert_with(|| toml::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(super::invalid)?;
    for path in accepted {
        if path.len() > 4096
            || !std::path::Path::new(path).is_absolute()
            || !path.ends_with("/SKILL.md")
            || path.contains("..")
        {
            return Err(super::invalid());
        }
        let matches: Vec<usize> = config
            .iter()
            .enumerate()
            .filter(|(_, v)| v.get("path").and_then(toml::Value::as_str) == Some(path))
            .map(|(i, _)| i)
            .collect();
        if matches.len() > 1 {
            return Err(super::invalid());
        }
        if let Some(index) = matches.first() {
            config[*index]
                .as_table_mut()
                .ok_or_else(super::invalid)?
                .insert("enabled".into(), toml::Value::Boolean(false));
        } else {
            config.push(toml::Value::Table(toml::map::Map::from_iter([
                ("path".into(), toml::Value::String(path.clone())),
                ("enabled".into(), toml::Value::Boolean(false)),
            ])));
        }
    }
    toml::to_string(&value).map_err(|_| super::invalid())
}
