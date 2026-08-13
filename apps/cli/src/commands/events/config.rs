use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use graphhelm_events::{KeyProvider, SecretBytes};
use graphhelm_postgres_event_store::backup::{DatabaseProcessProfile, PinnedTool};
use graphhelm_sealed_key_provider::SealedKeyProvider;
use serde::Deserialize;

use super::{Failure, argument, config_error};

/// Operator configuration is small by construction; anything larger is rejected unread.
const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_URL_BYTES: usize = 2048;
const MAX_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;
pub(super) const CONFIG_ENVIRONMENT: &str = "GRAPHHELM_EVENTS_CONFIG";
const KEY_ENVIRONMENT: &str = "GRAPHHELM_EVENTS_KEY";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawConfig {
    admin_url: String,
    passfile: PathBuf,
    keyring: RawKeyring,
    pg_dump: RawTool,
    pg_restore: RawTool,
    process_timeout_seconds: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawKeyring {
    directory: PathBuf,
    key_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawTool {
    path: PathBuf,
    sha256: String,
    version: String,
}

/// A validated operator configuration.
///
/// Every public surface of this type is redaction-safe: `Debug` never renders the DSN, the
/// passfile, or tool locations, and no loader error message quotes a path or credential.
pub(super) struct OperatorConfig {
    admin_url: String,
    profile: DatabaseProcessProfile,
    keyring_directory: PathBuf,
    key_id: String,
    pg_dump: PinnedTool,
    pg_restore: PinnedTool,
    process_timeout: Duration,
}

impl std::fmt::Debug for OperatorConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperatorConfig")
            .field("admin_url", &"[redacted]")
            .field("profile", &self.profile)
            .field("process_timeout", &self.process_timeout)
            .finish()
    }
}

impl OperatorConfig {
    pub(super) fn admin_url(&self) -> &str {
        &self.admin_url
    }

    pub(super) const fn process_timeout(&self) -> Duration {
        self.process_timeout
    }

    pub(super) fn into_tools(self) -> (DatabaseProcessProfile, PinnedTool, PinnedTool) {
        (self.profile, self.pg_dump, self.pg_restore)
    }

    /// Opens the sealed keyring.
    ///
    /// The 32-byte key never lives in the configuration file. It is supplied out of band through
    /// `GRAPHHELM_EVENTS_KEY` as 64 lowercase hexadecimal characters, so a leaked configuration
    /// alone cannot unwrap Evidence.
    pub(super) fn key_provider(&self) -> Result<Arc<dyn KeyProvider>, Failure> {
        let encoded = std::env::var(KEY_ENVIRONMENT).map_err(|_| {
            config_error(
                "GRAPHHELM_EVENTS_KEY must supply 64 lowercase hexadecimal characters",
                "/keyring",
            )
        })?;
        let material = decode_key(&encoded)?;
        let provider =
            SealedKeyProvider::open(&self.keyring_directory, self.key_id.clone(), material)
                .map_err(|_| config_error("the sealed keyring could not be opened", "/keyring"))?;
        Ok(Arc::new(provider))
    }
}

fn decode_key(encoded: &str) -> Result<SecretBytes, Failure> {
    let invalid = || {
        config_error(
            "GRAPHHELM_EVENTS_KEY must supply 64 lowercase hexadecimal characters",
            "/keyring",
        )
    };
    if encoded.len() != 64 {
        return Err(invalid());
    }
    let mut bytes = Vec::with_capacity(32);
    let raw = encoded.as_bytes();
    for pair in raw.chunks_exact(2) {
        let high = hex_value(pair[0]).ok_or_else(invalid)?;
        let low = hex_value(pair[1]).ok_or_else(invalid)?;
        bytes.push((high << 4) | low);
    }
    Ok(SecretBytes::new(bytes))
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Resolves the configuration path from an explicit flag or the environment, in that order.
pub(super) fn resolve_path(explicit: Option<&Path>) -> Result<PathBuf, Failure> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    match std::env::var_os(CONFIG_ENVIRONMENT) {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => Err(argument(
            "an operator configuration is required via --config or GRAPHHELM_EVENTS_CONFIG",
            "/config",
        )),
    }
}

pub(super) fn load(path: &Path) -> Result<OperatorConfig, Failure> {
    let raw = read_bounded(path)?;
    let parsed: RawConfig = serde_json::from_slice(&raw)
        .map_err(|_| config_error("operator configuration is not a valid JSON document", "/"))?;
    validate(parsed)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, Failure> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| config_error("operator configuration could not be read", "/"))?;
    if metadata.file_type().is_symlink() {
        return Err(config_error(
            "operator configuration must not be a symbolic link",
            "/",
        ));
    }
    if !metadata.is_file() {
        return Err(config_error(
            "operator configuration must be a regular file",
            "/",
        ));
    }
    reject_insecure_permissions(&metadata)?;
    if metadata.len() > MAX_CONFIG_BYTES as u64 {
        return Err(config_error(
            "operator configuration exceeds the maximum supported size",
            "/",
        ));
    }
    let file = File::open(path)
        .map_err(|_| config_error("operator configuration could not be read", "/"))?;
    let mut bytes = Vec::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| config_error("operator configuration could not be read", "/"))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(config_error(
            "operator configuration exceeds the maximum supported size",
            "/",
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn reject_insecure_permissions(metadata: &std::fs::Metadata) -> Result<(), Failure> {
    use std::os::unix::fs::MetadataExt;

    if metadata.mode() & 0o077 != 0 {
        return Err(config_error(
            "operator configuration must not be group or world accessible",
            "/",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_insecure_permissions(_: &std::fs::Metadata) -> Result<(), Failure> {
    // Windows ACL evaluation is not attempted; the symlink and regular-file rules still apply.
    Ok(())
}

fn validate(raw: RawConfig) -> Result<OperatorConfig, Failure> {
    let RawConfig {
        admin_url,
        passfile,
        keyring,
        pg_dump,
        pg_restore,
        process_timeout_seconds,
    } = raw;
    if process_timeout_seconds == 0 || process_timeout_seconds > MAX_TIMEOUT_SECONDS {
        return Err(config_error(
            "processTimeoutSeconds must be between 1 and 86400",
            "/processTimeoutSeconds",
        ));
    }
    let endpoint = parse_admin_url(&admin_url)?;
    if !passfile.is_absolute() {
        return Err(config_error(
            "passfile must be an absolute path",
            "/passfile",
        ));
    }
    let profile = DatabaseProcessProfile::new(
        endpoint.host,
        endpoint.port,
        endpoint.user,
        endpoint.database,
        passfile,
    )
    .map_err(|_| {
        config_error(
            "adminUrl and passfile do not describe a usable database endpoint",
            "/adminUrl",
        )
    })?;
    if !keyring.directory.is_absolute() {
        return Err(config_error(
            "keyring directory must be an absolute path",
            "/keyring/directory",
        ));
    }
    if keyring.key_id.is_empty() || keyring.key_id.len() > 128 {
        return Err(config_error(
            "keyring keyId must be a bounded identifier",
            "/keyring/keyId",
        ));
    }
    let pg_dump = pinned(pg_dump, "/pgDump")?;
    let pg_restore = pinned(pg_restore, "/pgRestore")?;
    Ok(OperatorConfig {
        admin_url,
        profile,
        keyring_directory: keyring.directory,
        key_id: keyring.key_id,
        pg_dump,
        pg_restore,
        process_timeout: Duration::from_secs(process_timeout_seconds),
    })
}

fn pinned(tool: RawTool, pointer: &str) -> Result<PinnedTool, Failure> {
    if !tool.path.is_absolute() {
        return Err(config_error(
            "tool path must be absolute",
            &format!("{pointer}/path"),
        ));
    }
    if tool.sha256.len() != 64
        || !tool
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(config_error(
            "tool sha256 must be 64 lowercase hexadecimal characters",
            &format!("{pointer}/sha256"),
        ));
    }
    PinnedTool::new(tool.path, &tool.sha256, tool.version)
        .map_err(|_| config_error("tool pin is not usable", pointer))
}

struct Endpoint {
    host: String,
    port: u16,
    user: String,
    database: String,
}

/// Strict, bounded `postgres://` parser.
///
/// The DSN is never echoed back; every failure reports the same generic message so that neither the
/// host, the user, nor an embedded password can be reconstructed from CLI output.
fn parse_admin_url(value: &str) -> Result<Endpoint, Failure> {
    let invalid = || config_error("adminUrl is not a supported postgres:// DSN", "/adminUrl");
    if value.len() > MAX_URL_BYTES {
        return Err(invalid());
    }
    let rest = value
        .strip_prefix("postgres://")
        .or_else(|| value.strip_prefix("postgresql://"))
        .ok_or_else(invalid)?;
    let (authority, database) = rest.split_once('/').ok_or_else(invalid)?;
    if database.is_empty() || database.contains('?') || database.contains('/') {
        return Err(invalid());
    }
    let (credentials, host_port) = match authority.rsplit_once('@') {
        Some((credentials, host_port)) => (credentials, host_port),
        None => return Err(invalid()),
    };
    let user = credentials
        .split_once(':')
        .map_or(credentials, |(user, _)| user);
    if user.is_empty() {
        return Err(invalid());
    }
    let (host, port) = host_port.rsplit_once(':').ok_or_else(invalid)?;
    let port = port.parse::<u16>().map_err(|_| invalid())?;
    if host.is_empty() {
        return Err(invalid());
    }
    Ok(Endpoint {
        host: host.to_owned(),
        port,
        user: user.to_owned(),
        database: database.to_owned(),
    })
}
