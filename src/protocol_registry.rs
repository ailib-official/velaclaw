//! Scan `AI_PROTOCOL_DIR` for provider manifests and model registry entries.
//! Used by CLI `models protocol-*` and availability checks.

use ai_lib_rust::protocol::ProtocolManifest;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ENV_PROTOCOL_DIR: &str = "AI_PROTOCOL_DIR";
const ENV_PROTOCOL_PATH: &str = "AI_PROTOCOL_PATH";

/// Parse a value of `AI_PROTOCOL_DIR` / `AI_PROTOCOL_PATH`.
///
/// Returns a directory only for **local** paths (not `http`/`https` URLs) that exist on disk.
/// Used by the onboard wizard, CLI, and tests so rules stay in one place.
pub fn protocol_root_from_path_value(raw: &str) -> Option<PathBuf> {
    let t = raw.trim();
    if t.is_empty() || t.starts_with("http://") || t.starts_with("https://") {
        return None;
    }
    let p = PathBuf::from(t);
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// Resolve local ai-protocol checkout root (not HTTP URLs).
pub fn resolve_local_protocol_root() -> Option<PathBuf> {
    let raw = std::env::var(ENV_PROTOCOL_DIR)
        .ok()
        .or_else(|| std::env::var(ENV_PROTOCOL_PATH).ok())?;
    protocol_root_from_path_value(&raw)
}

fn collect_provider_files(root: &Path) -> Vec<PathBuf> {
    // Higher-priority directories first; one manifest per provider stem.
    let candidates = [
        root.join("dist").join("v2").join("providers"),
        root.join("v2").join("providers"),
        root.join("dist").join("v1").join("providers"),
        root.join("v1").join("providers"),
    ];
    let mut by_stem: BTreeMap<String, PathBuf> = BTreeMap::new();
    for dir in candidates {
        if !dir.is_dir() {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            let ext = path.extension().and_then(|s| s.to_str());
            let ok = path.is_file() && matches!(ext, Some("json" | "yaml" | "yml"));
            if !ok {
                continue;
            }
            let Some(stem) = provider_id_from_path(&path) else {
                continue;
            };
            by_stem.entry(stem).or_insert(path);
        }
    }
    by_stem.into_values().collect()
}

fn provider_id_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(std::string::ToString::to_string)
}

fn load_provider_manifest(path: &Path) -> anyhow::Result<ProtocolManifest> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let bytes = std::fs::read(path)?;
    if ext.eq_ignore_ascii_case("json") {
        return Ok(serde_json::from_slice(&bytes)?);
    }
    if ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml") {
        let s = String::from_utf8_lossy(&bytes);
        return Ok(serde_yaml::from_str(&s)?);
    }
    anyhow::bail!("unsupported provider manifest extension: {ext}");
}

/// One provider from disk with optional auth env analysis.
#[derive(Debug, Clone, Serialize)]
pub struct ProtocolProviderInfo {
    pub id: String,
    pub manifest_path: PathBuf,
    pub required_envs: Vec<String>,
    pub available: bool,
}

/// Logical model id from a registry file (`models` map keys + provider field).
#[derive(Debug, Clone, Serialize)]
pub struct ProtocolModelInfo {
    pub logical_id: String,
    pub provider: String,
    pub source_file: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProtocolRegistrySnapshot {
    pub protocol_root: PathBuf,
    pub providers: Vec<ProtocolProviderInfo>,
    pub models: Vec<ProtocolModelInfo>,
}

fn context_window_from_meta(meta: &serde_json::Value) -> Option<u32> {
    meta.get("context_window")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

fn load_manifest_value(path: &Path) -> anyhow::Result<serde_json::Value> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let bytes = std::fs::read(path)?;
    if ext.eq_ignore_ascii_case("json") {
        return Ok(serde_json::from_slice(&bytes)?);
    }
    if ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml") {
        let s = String::from_utf8_lossy(&bytes);
        let v: serde_yaml::Value = serde_yaml::from_str(&s)?;
        return Ok(serde_json::to_value(v)?);
    }
    anyhow::bail!("unsupported provider manifest extension: {ext}");
}

fn upsert_model(models: &mut Vec<ProtocolModelInfo>, entry: ProtocolModelInfo) {
    if let Some(existing) = models.iter_mut().find(|m| m.logical_id == entry.logical_id) {
        if existing.context_window.is_none() {
            existing.context_window = entry.context_window;
        }
        return;
    }
    models.push(entry);
}

fn ingest_provider_metadata_models(
    models: &mut Vec<ProtocolModelInfo>,
    provider_id: &str,
    path: &Path,
) -> bool {
    let Ok(raw) = load_manifest_value(path) else {
        return false;
    };
    let Some(metadata_models) = raw
        .get("metadata")
        .and_then(|m| m.get("models"))
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    for (model_key, meta) in metadata_models {
        let logical_id = if model_key.contains('/') {
            model_key.clone()
        } else {
            format!("{provider_id}/{model_key}")
        };
        upsert_model(
            models,
            ProtocolModelInfo {
                logical_id,
                provider: provider_id.to_string(),
                source_file: path.to_path_buf(),
                context_window: context_window_from_meta(meta),
            },
        );
    }
    true
}

fn provider_id_from_manifest_value(raw: &serde_json::Value, stem: &str) -> String {
    raw.get("id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(stem)
        .to_string()
}

impl ProtocolRegistrySnapshot {
    /// Resolve `context_window` tokens for a logical model id (exact or suffix match).
    #[must_use]
    pub fn context_window_for(&self, model_id: &str) -> Option<u32> {
        let model_id = model_id.trim();
        if model_id.is_empty() {
            return None;
        }
        if let Some(found) = self
            .models
            .iter()
            .find(|m| m.logical_id == model_id)
            .and_then(|m| m.context_window)
        {
            return Some(found);
        }
        let suffix = model_id.rsplit('/').next().unwrap_or(model_id);
        self.models
            .iter()
            .filter(|m| {
                m.logical_id == suffix
                    || m.logical_id.ends_with(&format!("/{suffix}"))
                    || m.logical_id == format!("{}/{}", m.provider, suffix)
            })
            .find_map(|m| m.context_window)
    }

    /// Find a provider by id (case-sensitive exact match).
    #[must_use]
    pub fn provider_by_id(&self, provider_id: &str) -> Option<&ProtocolProviderInfo> {
        let provider_id = provider_id.trim();
        self.providers.iter().find(|p| p.id == provider_id)
    }

    /// Resolve a logical model id (exact, or `provider/model` suffix forms).
    #[must_use]
    pub fn model_by_logical_id(&self, model_id: &str) -> Option<&ProtocolModelInfo> {
        let model_id = model_id.trim();
        if model_id.is_empty() {
            return None;
        }
        if let Some(found) = self.models.iter().find(|m| m.logical_id == model_id) {
            return Some(found);
        }
        let suffix = model_id.rsplit('/').next().unwrap_or(model_id);
        self.models.iter().find(|m| {
            m.logical_id == suffix
                || m.logical_id.ends_with(&format!("/{suffix}"))
                || m.logical_id == format!("{}/{}", m.provider, suffix)
        })
    }
}

/// Whether a provider manifest exposes a chat-capable endpoint key.
///
/// Accepts `endpoints.chat` or `endpoints.chat_openai` (ai-lib chat op aliases).
/// Returns `None` when the file cannot be parsed or has no `endpoints` map.
#[must_use]
pub fn manifest_has_chat_endpoint(path: &Path) -> Option<bool> {
    let raw = load_manifest_value(path).ok()?;
    let endpoints = raw.get("endpoints")?.as_object()?;
    Some(endpoints.contains_key("chat") || endpoints.contains_key("chat_openai"))
}

/// Provider id segment from `provider`, `provider/model`, or `protocol:provider/model`.
#[must_use]
pub fn provider_id_from_logical(raw: &str) -> &str {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix("protocol:")
        .map(str::trim)
        .unwrap_or(raw);
    raw.split_once('/').map(|(p, _)| p).unwrap_or(raw)
}

/// Lookup `context_window` for a model from the local ai-protocol registry cache.
#[must_use]
pub fn lookup_context_window(model_id: &str) -> Option<u32> {
    #[cfg(feature = "ai-protocol")]
    {
        use std::sync::OnceLock;
        static CACHE: OnceLock<Option<ProtocolRegistrySnapshot>> = OnceLock::new();
        let snap = CACHE.get_or_init(|| {
            resolve_local_protocol_root().and_then(|root| scan_protocol_root(&root).ok())
        });
        snap.as_ref()?.context_window_for(model_id)
    }
    #[cfg(not(feature = "ai-protocol"))]
    {
        let _ = model_id;
        None
    }
}

/// Scan provider manifests under `root` and model registries under `v1/models` / `dist/v1/models`.
pub fn scan_protocol_root(root: &Path) -> anyhow::Result<ProtocolRegistrySnapshot> {
    let mut providers = Vec::new();
    let mut models = Vec::new();
    for path in collect_provider_files(root) {
        let Some(stem_id) = provider_id_from_path(&path) else {
            continue;
        };
        match load_provider_manifest(&path) {
            Ok(manifest) => {
                let required_envs = ai_lib_rust::credentials::required_envs(&manifest);
                let has_auth = ai_lib_rust::credentials::primary_auth(&manifest).is_some();
                let available = !has_auth
                    || ai_lib_rust::credentials::resolve_credential(&manifest, None)
                        .secret()
                        .is_some();
                let resolved_id = if manifest.id.trim().is_empty() {
                    stem_id.clone()
                } else {
                    manifest.id.clone()
                };
                providers.push(ProtocolProviderInfo {
                    id: resolved_id.clone(),
                    manifest_path: path.clone(),
                    required_envs,
                    available,
                });
                ingest_provider_metadata_models(&mut models, &resolved_id, &path);
            }
            Err(e) => {
                let Ok(raw) = load_manifest_value(&path) else {
                    tracing::warn!(path = %path.display(), "skip invalid provider manifest: {e}");
                    continue;
                };
                let resolved_id = provider_id_from_manifest_value(&raw, &stem_id);
                if ingest_provider_metadata_models(&mut models, &resolved_id, &path) {
                    tracing::debug!(
                        path = %path.display(),
                        provider = %resolved_id,
                        error = %e,
                        "provider manifest skipped strict validation; indexed metadata.models only"
                    );
                } else {
                    tracing::warn!(path = %path.display(), "skip invalid provider manifest: {e}");
                }
            }
        }
    }
    providers.sort_by(|a, b| a.id.cmp(&b.id));

    for base in [
        root.join("dist").join("v1").join("models"),
        root.join("v1").join("models"),
    ] {
        if !base.is_dir() {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&base) else {
            continue;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            let ext = path.extension().and_then(|s| s.to_str());
            let prefer_json = ext == Some("json");
            let prefer_yaml = matches!(ext, Some("yaml" | "yml"));
            if !(prefer_json || prefer_yaml) {
                continue;
            }
            let reg: BTreeMap<String, serde_json::Value> = if prefer_json {
                let bytes = match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                let v: serde_json::Value = match serde_json::from_slice(&bytes) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let Some(m) = v.get("models").and_then(|x| x.as_object()) else {
                    continue;
                };
                m.iter().map(|(k, val)| (k.clone(), val.clone())).collect()
            } else {
                let s = match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let v: serde_yaml::Value = match serde_yaml::from_str(&s) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let Some(m) = v.get("models").and_then(|x| x.as_mapping()) else {
                    continue;
                };
                let mut out = BTreeMap::new();
                for (k, val) in m {
                    let Some(ks) = k.as_str() else {
                        continue;
                    };
                    let j = serde_json::to_value(val).unwrap_or(serde_json::Value::Null);
                    out.insert(ks.to_string(), j);
                }
                out
            };

            for (logical_id, meta) in reg {
                let provider = meta
                    .get("provider")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                upsert_model(
                    &mut models,
                    ProtocolModelInfo {
                        logical_id,
                        provider,
                        source_file: path.clone(),
                        context_window: context_window_from_meta(&meta),
                    },
                );
            }
        }
    }
    models.sort_by(|a, b| a.logical_id.cmp(&b.logical_id));

    Ok(ProtocolRegistrySnapshot {
        protocol_root: root.to_path_buf(),
        providers,
        models,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        old: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let old = std::env::var(key).ok();
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.old.as_ref() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn scan_empty_dir_yields_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let snap = scan_protocol_root(dir.path()).expect("scan");
        assert!(snap.providers.is_empty());
        assert!(snap.models.is_empty());
    }

    #[test]
    fn protocol_root_from_path_rejects_http_urls() {
        assert!(protocol_root_from_path_value("https://example.com/proto").is_none());
        assert!(protocol_root_from_path_value("http://localhost/x").is_none());
    }

    #[test]
    fn protocol_root_from_path_accepts_existing_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        let got = protocol_root_from_path_value(p.to_str().expect("utf8 path"));
        assert_eq!(got.as_deref(), Some(p));
    }

    #[test]
    fn scan_provider_uses_ai_lib_endpoint_auth_availability() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _env = EnvGuard::set("VELACLAW_PT074_TOKEN", Some("test-token"));
        let dir = tempfile::tempdir().expect("tempdir");
        let providers = dir.path().join("v2").join("providers");
        fs::create_dir_all(&providers).expect("provider dir");
        fs::write(
            providers.join("pt074.yaml"),
            r#"
id: pt074
protocol_version: v2-alpha
provider_id: pt074-provider
name: PT-074 Provider
version: v2
status: stable
category: ai_provider
official_url: https://example.com
support_contact: support@example.com
capabilities: [chat]
endpoint:
  base_url: https://example.com/v1
  auth:
    type: bearer
    token_env: VELACLAW_PT074_TOKEN
"#,
        )
        .expect("manifest");

        let snap = scan_protocol_root(dir.path()).expect("scan");
        let provider = snap
            .providers
            .iter()
            .find(|provider| provider.id == "pt074")
            .expect("provider");
        assert_eq!(provider.required_envs, vec!["VELACLAW_PT074_TOKEN"]);
        assert!(provider.available);
    }

    #[test]
    fn scan_provider_uses_ai_lib_conventional_env_fallback() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _manifest_env = EnvGuard::set("VELACLAW_PT074_MISSING_TOKEN", None);
        let _conventional_env = EnvGuard::set("PT074_PROVIDER_API_KEY", Some("test-token"));
        let dir = tempfile::tempdir().expect("tempdir");
        let providers = dir.path().join("v2").join("providers");
        fs::create_dir_all(&providers).expect("provider dir");
        fs::write(
            providers.join("pt074.yaml"),
            r#"
id: pt074
protocol_version: v2-alpha
provider_id: pt074-provider
name: PT-074 Provider
version: v2
status: stable
category: ai_provider
official_url: https://example.com
support_contact: support@example.com
capabilities: [chat]
endpoint:
  base_url: https://example.com/v1
  auth:
    type: bearer
    token_env: VELACLAW_PT074_MISSING_TOKEN
"#,
        )
        .expect("manifest");

        let snap = scan_protocol_root(dir.path()).expect("scan");
        let provider = snap
            .providers
            .iter()
            .find(|provider| provider.id == "pt074")
            .expect("provider");
        assert_eq!(provider.required_envs, vec!["VELACLAW_PT074_MISSING_TOKEN"]);
        assert!(provider.available);
    }

    #[test]
    fn scan_lenient_manifest_without_status_indexes_metadata_models() {
        let dir = tempfile::tempdir().expect("tempdir");
        let providers = dir.path().join("v2").join("providers");
        fs::create_dir_all(&providers).expect("provider dir");
        fs::write(
            providers.join("azure.yaml"),
            r#"
id: azure
name: Azure
metadata:
  models:
    gpt-4o:
      context_window: 128000
"#,
        )
        .expect("manifest");

        let snap = scan_protocol_root(dir.path()).expect("scan");
        assert!(
            snap.providers.is_empty(),
            "strict parse should skip provider entry without status"
        );
        assert_eq!(snap.context_window_for("azure/gpt-4o"), Some(128_000));
    }

    #[test]
    fn scan_provider_metadata_models_extracts_context_window() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ai-protocol-min");
        let snap = scan_protocol_root(&fixture).expect("scan fixture");
        let cw = snap
            .context_window_for("openai/gpt-5.3-codex-spark")
            .or_else(|| snap.context_window_for("gpt-5.3-codex-spark"));
        assert_eq!(cw, Some(128_000));
    }

    #[test]
    fn provider_and_model_lookup_helpers() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ai-protocol-min");
        let snap = scan_protocol_root(&fixture).expect("scan fixture");
        assert!(snap.provider_by_id("openai").is_some());
        assert!(snap
            .model_by_logical_id("openai/gpt-5.3-codex-spark")
            .is_some());
        assert_eq!(
            provider_id_from_logical("deepseek/deepseek-v4-flash"),
            "deepseek"
        );
        assert_eq!(provider_id_from_logical("deepseek"), "deepseek");
        assert_eq!(
            provider_id_from_logical("protocol:openai/gpt-5.2"),
            "openai"
        );
        assert_eq!(provider_id_from_logical("protocol:deepseek"), "deepseek");
    }

    #[test]
    fn manifest_has_chat_endpoint_detects_chat_keys() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ai-protocol-min/v2/providers/openai.yaml");
        assert_eq!(manifest_has_chat_endpoint(&fixture), Some(true));

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nochat.yaml");
        fs::write(
            &path,
            r#"
id: nochat
endpoints:
  embeddings:
    path: /v1/embeddings
"#,
        )
        .expect("write");
        assert_eq!(manifest_has_chat_endpoint(&path), Some(false));
    }
}
