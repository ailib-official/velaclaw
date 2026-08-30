use super::hint_peer::{
    admit_attempt, is_peer_switchable, ordered_candidates, provider_family, Admit,
    HintPeerCandidate, HintPeerSession,
};
use super::traits::{ChatMessage, ChatRequest, ChatResponse};
use super::Provider;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

/// A single route: maps a task hint to a provider + model combo.
#[derive(Debug, Clone, Default)]
pub struct Route {
    pub provider_name: String,
    pub model: String,
    pub fallbacks: Vec<(String, String)>,
}

struct ResolvedHint {
    idx: usize,
    model: String,
    chain: Vec<HintPeerCandidate>,
}

/// Multi-model router — routes requests to different provider+model combos
/// based on a task hint encoded in the model parameter.
///
/// The model parameter can be:
/// - A regular model name (e.g. "anthropic/claude-sonnet-4") → uses default provider
/// - A hint-prefixed string (e.g. "hint:reasoning") → resolves via route table
///
/// This wraps multiple pre-created providers and selects the right one per request.
pub struct RouterProvider {
    routes: HashMap<String, ResolvedHint>,
    providers: Vec<(String, Box<dyn Provider>)>,
    default_index: usize,
    #[allow(dead_code)]
    default_model: String,
    hint_peer_fallback: bool,
    peer: Mutex<HintPeerSession>,
}

impl RouterProvider {
    /// Create a new router with a default provider and optional routes.
    ///
    /// `providers` is a list of (name, provider) pairs. The first one is the default.
    /// `routes` maps hint names to Route structs containing provider_name and model.
    pub fn new(
        providers: Vec<(String, Box<dyn Provider>)>,
        routes: Vec<(String, Route)>,
        default_model: String,
    ) -> Self {
        let name_to_index: HashMap<String, usize> = providers
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (name.clone(), i))
            .collect();

        let resolved_routes: HashMap<String, ResolvedHint> = routes
            .into_iter()
            .filter_map(|(hint, route)| {
                let index = name_to_index.get(&route.provider_name).copied();
                match index {
                    Some(i) => {
                        let mut chain = vec![HintPeerCandidate {
                            provider_name: route.provider_name.clone(),
                            model: route.model.clone(),
                        }];
                        for (p, m) in route.fallbacks {
                            if name_to_index.contains_key(&p) {
                                chain.push(HintPeerCandidate {
                                    provider_name: p,
                                    model: m,
                                });
                            } else {
                                tracing::warn!(
                                    hint = hint.as_str(),
                                    provider = p.as_str(),
                                    "Hint peer fallback references unknown provider, skipping"
                                );
                            }
                        }
                        Some((
                            hint,
                            ResolvedHint {
                                idx: i,
                                model: route.model,
                                chain,
                            },
                        ))
                    }
                    None => {
                        tracing::warn!(
                            hint = hint,
                            provider = route.provider_name,
                            "Route references unknown provider, skipping"
                        );
                        None
                    }
                }
            })
            .collect();

        Self {
            routes: resolved_routes,
            providers,
            default_index: 0,
            default_model,
            hint_peer_fallback: false,
            peer: Mutex::new(HintPeerSession::default()),
        }
    }

    #[must_use]
    pub fn with_hint_peer_fallback(mut self, enabled: bool) -> Self {
        self.hint_peer_fallback = enabled;
        self
    }

    fn resolve(&self, model: &str) -> (usize, String) {
        if let Some(hint) = model.strip_prefix("hint:") {
            if self.hint_peer_fallback {
                if let Some(pinned) = self.pinned_for(hint) {
                    if let Some(idx) = self.index_for_model(hint, &pinned) {
                        return (idx, pinned);
                    }
                }
            }
            if let Some(route) = self.routes.get(hint) {
                return (route.idx, route.model.clone());
            }
            tracing::warn!(
                hint = hint,
                "Unknown route hint, falling back to default provider"
            );
        }
        (self.default_index, model.to_string())
    }

    fn pinned_for(&self, hint: &str) -> Option<String> {
        self.peer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pinned
            .get(hint)
            .cloned()
    }

    fn index_for_model(&self, hint: &str, model: &str) -> Option<usize> {
        let route = self.routes.get(hint)?;
        let name = route
            .chain
            .iter()
            .find(|c| c.model == model)
            .map(|c| c.provider_name.as_str())?;
        self.providers.iter().position(|(n, _)| n == name)
    }

    fn hop_candidates(&self, model: &str) -> Vec<(usize, String, String)> {
        let (idx, resolved) = self.resolve(model);
        let name = self
            .providers
            .get(idx)
            .map(|(n, _)| n.clone())
            .unwrap_or_default();
        if !self.hint_peer_fallback {
            return vec![(idx, name, resolved)];
        }
        let Some(hint) = model.strip_prefix("hint:") else {
            return vec![(idx, name, resolved)];
        };
        let Some(route) = self.routes.get(hint) else {
            return vec![(idx, name, resolved)];
        };
        let session = self.peer.lock().unwrap_or_else(|e| e.into_inner());
        let ordered = ordered_candidates(hint, &route.chain, &session);
        drop(session);
        ordered
            .into_iter()
            .filter_map(|c| {
                let idx = self
                    .providers
                    .iter()
                    .position(|(n, _)| n == &c.provider_name)?;
                Some((idx, c.provider_name, c.model))
            })
            .collect()
    }

    fn record_success(&self, requested: &str, used_model: &str) {
        let Some(hint) = requested.strip_prefix("hint:") else {
            return;
        };
        self.peer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pin(hint, used_model);
    }

    fn record_failure(&self, used_model: &str) {
        self.peer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .blacklist(used_model);
    }
}

#[async_trait]
impl Provider for RouterProvider {
    fn routed_model_label(&self, requested: &str) -> String {
        let (_, model) = self.resolve(requested);
        model
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let mut last_err: Option<anyhow::Error> = None;
        let mut prev_family: Option<String> = None;
        let mut attempts = 0usize;
        let mut cross = 0usize;
        for (idx, prov_name, resolved) in self.hop_candidates(model) {
            match admit_attempt(
                prev_family.as_deref(),
                provider_family(&prov_name),
                attempts,
                cross,
            ) {
                Admit::Stop => break,
                Admit::Skip => continue,
                Admit::Take { cross_delta } => {
                    attempts += 1;
                    cross += cross_delta;
                    prev_family = Some(provider_family(&prov_name).to_string());
                    tracing::info!(
                        provider = prov_name.as_str(),
                        model = resolved.as_str(),
                        "Router dispatching request"
                    );
                    match self.providers[idx]
                        .1
                        .chat_with_system(system_prompt, message, &resolved, temperature)
                        .await
                    {
                        Ok(v) => {
                            if attempts > 1 {
                                tracing::info!(
                                    hint = model,
                                    to = resolved.as_str(),
                                    "hint_peer_fallback succeeded"
                                );
                            }
                            self.record_success(model, &resolved);
                            return Ok(v);
                        }
                        Err(e) => {
                            if !self.hint_peer_fallback || !is_peer_switchable(&e.to_string()) {
                                return Err(e);
                            }
                            tracing::warn!(
                                provider = prov_name.as_str(),
                                model = resolved.as_str(),
                                error = %crate::providers::sanitize_api_error(&e.to_string()),
                                "hint_peer_fallback: blacklisting model and trying next"
                            );
                            self.record_failure(&resolved);
                            last_err = Some(e);
                        }
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no hint peer candidates for {model}")))
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let mut last_err: Option<anyhow::Error> = None;
        let mut prev_family: Option<String> = None;
        let mut attempts = 0usize;
        let mut cross = 0usize;
        for (idx, prov_name, resolved) in self.hop_candidates(model) {
            match admit_attempt(
                prev_family.as_deref(),
                provider_family(&prov_name),
                attempts,
                cross,
            ) {
                Admit::Stop => break,
                Admit::Skip => continue,
                Admit::Take { cross_delta } => {
                    attempts += 1;
                    cross += cross_delta;
                    prev_family = Some(provider_family(&prov_name).to_string());
                    match self.providers[idx]
                        .1
                        .chat_with_history(messages, &resolved, temperature)
                        .await
                    {
                        Ok(v) => {
                            self.record_success(model, &resolved);
                            return Ok(v);
                        }
                        Err(e) => {
                            if !self.hint_peer_fallback || !is_peer_switchable(&e.to_string()) {
                                return Err(e);
                            }
                            self.record_failure(&resolved);
                            last_err = Some(e);
                        }
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no hint peer candidates for {model}")))
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let mut last_err: Option<anyhow::Error> = None;
        let mut prev_family: Option<String> = None;
        let mut attempts = 0usize;
        let mut cross = 0usize;
        for (idx, prov_name, resolved) in self.hop_candidates(model) {
            match admit_attempt(
                prev_family.as_deref(),
                provider_family(&prov_name),
                attempts,
                cross,
            ) {
                Admit::Stop => break,
                Admit::Skip => continue,
                Admit::Take { cross_delta } => {
                    attempts += 1;
                    cross += cross_delta;
                    prev_family = Some(provider_family(&prov_name).to_string());
                    match self.providers[idx]
                        .1
                        .chat(request, &resolved, temperature)
                        .await
                    {
                        Ok(v) => {
                            self.record_success(model, &resolved);
                            return Ok(v);
                        }
                        Err(e) => {
                            if !self.hint_peer_fallback || !is_peer_switchable(&e.to_string()) {
                                return Err(e);
                            }
                            self.record_failure(&resolved);
                            last_err = Some(e);
                        }
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no hint peer candidates for {model}")))
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let mut last_err: Option<anyhow::Error> = None;
        let mut prev_family: Option<String> = None;
        let mut attempts = 0usize;
        let mut cross = 0usize;
        for (idx, prov_name, resolved) in self.hop_candidates(model) {
            match admit_attempt(
                prev_family.as_deref(),
                provider_family(&prov_name),
                attempts,
                cross,
            ) {
                Admit::Stop => break,
                Admit::Skip => continue,
                Admit::Take { cross_delta } => {
                    attempts += 1;
                    cross += cross_delta;
                    prev_family = Some(provider_family(&prov_name).to_string());
                    match self.providers[idx]
                        .1
                        .chat_with_tools(messages, tools, &resolved, temperature)
                        .await
                    {
                        Ok(v) => {
                            self.record_success(model, &resolved);
                            return Ok(v);
                        }
                        Err(e) => {
                            if !self.hint_peer_fallback || !is_peer_switchable(&e.to_string()) {
                                return Err(e);
                            }
                            self.record_failure(&resolved);
                            last_err = Some(e);
                        }
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no hint peer candidates for {model}")))
    }

    fn supports_native_tools(&self) -> bool {
        self.providers
            .get(self.default_index)
            .map(|(_, p)| p.supports_native_tools())
            .unwrap_or(false)
    }

    fn supports_vision(&self) -> bool {
        self.providers
            .iter()
            .any(|(_, provider)| provider.supports_vision())
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        for (name, provider) in &self.providers {
            tracing::info!(provider = name, "Warming up routed provider");
            if let Err(e) = provider.warmup().await {
                tracing::warn!(provider = name, "Warmup failed (non-fatal): {e}");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct MockProvider {
        calls: Arc<AtomicUsize>,
        response: &'static str,
        last_model: parking_lot::Mutex<String>,
    }

    impl MockProvider {
        fn new(response: &'static str) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                response,
                last_model: parking_lot::Mutex::new(String::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_model(&self) -> String {
            self.last_model.lock().clone()
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_model.lock() = model.to_string();
            Ok(self.response.to_string())
        }
    }

    fn make_router(
        providers: Vec<(&'static str, &'static str)>,
        routes: Vec<(&str, &str, &str)>,
    ) -> (RouterProvider, Vec<Arc<MockProvider>>) {
        let mocks: Vec<Arc<MockProvider>> = providers
            .iter()
            .map(|(_, response)| Arc::new(MockProvider::new(response)))
            .collect();

        let provider_list: Vec<(String, Box<dyn Provider>)> = providers
            .iter()
            .zip(mocks.iter())
            .map(|((name, _), mock)| {
                (
                    name.to_string(),
                    Box::new(Arc::clone(mock)) as Box<dyn Provider>,
                )
            })
            .collect();

        let route_list: Vec<(String, Route)> = routes
            .iter()
            .map(|(hint, provider_name, model)| {
                (
                    hint.to_string(),
                    Route {
                        provider_name: provider_name.to_string(),
                        model: model.to_string(),
                        fallbacks: Vec::new(),
                    },
                )
            })
            .collect();

        let router = RouterProvider::new(provider_list, route_list, "default-model".to_string());

        (router, mocks)
    }

    // Arc<MockProvider> should also be a Provider
    #[async_trait]
    impl Provider for Arc<MockProvider> {
        async fn chat_with_system(
            &self,
            system_prompt: Option<&str>,
            message: &str,
            model: &str,
            temperature: f64,
        ) -> anyhow::Result<String> {
            self.as_ref()
                .chat_with_system(system_prompt, message, model, temperature)
                .await
        }
    }

    #[tokio::test]
    async fn routes_hint_to_correct_provider() {
        let (router, mocks) = make_router(
            vec![("fast", "fast-response"), ("smart", "smart-response")],
            vec![
                ("fast", "fast", "llama-3-70b"),
                ("reasoning", "smart", "claude-opus"),
            ],
        );

        let result = router
            .simple_chat("hello", "hint:reasoning", 0.5)
            .await
            .unwrap();
        assert_eq!(result, "smart-response");
        assert_eq!(mocks[1].call_count(), 1);
        assert_eq!(mocks[1].last_model(), "claude-opus");
        assert_eq!(mocks[0].call_count(), 0);
    }

    #[tokio::test]
    async fn routes_fast_hint() {
        let (router, mocks) = make_router(
            vec![("fast", "fast-response"), ("smart", "smart-response")],
            vec![("fast", "fast", "llama-3-70b")],
        );

        let result = router.simple_chat("hello", "hint:fast", 0.5).await.unwrap();
        assert_eq!(result, "fast-response");
        assert_eq!(mocks[0].call_count(), 1);
        assert_eq!(mocks[0].last_model(), "llama-3-70b");
    }

    #[tokio::test]
    async fn unknown_hint_falls_back_to_default() {
        let (router, mocks) = make_router(
            vec![("default", "default-response"), ("other", "other-response")],
            vec![],
        );

        let result = router
            .simple_chat("hello", "hint:nonexistent", 0.5)
            .await
            .unwrap();
        assert_eq!(result, "default-response");
        assert_eq!(mocks[0].call_count(), 1);
        // Falls back to default with the hint as model name
        assert_eq!(mocks[0].last_model(), "hint:nonexistent");
    }

    #[tokio::test]
    async fn non_hint_model_uses_default_provider() {
        let (router, mocks) = make_router(
            vec![
                ("primary", "primary-response"),
                ("secondary", "secondary-response"),
            ],
            vec![("code", "secondary", "codellama")],
        );

        let result = router
            .simple_chat("hello", "anthropic/claude-sonnet-4-20250514", 0.5)
            .await
            .unwrap();
        assert_eq!(result, "primary-response");
        assert_eq!(mocks[0].call_count(), 1);
        assert_eq!(mocks[0].last_model(), "anthropic/claude-sonnet-4-20250514");
    }

    #[test]
    fn resolve_preserves_model_for_non_hints() {
        let (router, _) = make_router(vec![("default", "ok")], vec![]);

        let (idx, model) = router.resolve("gpt-4o");
        assert_eq!(idx, 0);
        assert_eq!(model, "gpt-4o");
    }

    #[test]
    fn resolve_strips_hint_prefix() {
        let (router, _) = make_router(
            vec![("fast", "ok"), ("smart", "ok")],
            vec![("reasoning", "smart", "claude-opus")],
        );

        let (idx, model) = router.resolve("hint:reasoning");
        assert_eq!(idx, 1);
        assert_eq!(model, "claude-opus");
    }

    #[test]
    fn skips_routes_with_unknown_provider() {
        let (router, _) = make_router(
            vec![("default", "ok")],
            vec![("broken", "nonexistent", "model")],
        );

        // Route should not exist
        assert!(!router.routes.contains_key("broken"));
    }

    #[tokio::test]
    async fn warmup_calls_all_providers() {
        let (router, _) = make_router(vec![("a", "ok"), ("b", "ok")], vec![]);

        // Warmup should not error
        assert!(router.warmup().await.is_ok());
    }

    #[tokio::test]
    async fn chat_with_system_passes_system_prompt() {
        let mock = Arc::new(MockProvider::new("response"));
        let router = RouterProvider::new(
            vec![(
                "default".into(),
                Box::new(Arc::clone(&mock)) as Box<dyn Provider>,
            )],
            vec![],
            "model".into(),
        );

        let result = router
            .chat_with_system(Some("system"), "hello", "model", 0.5)
            .await
            .unwrap();
        assert_eq!(result, "response");
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn chat_with_tools_delegates_to_resolved_provider() {
        let mock = Arc::new(MockProvider::new("tool-response"));
        let router = RouterProvider::new(
            vec![(
                "default".into(),
                Box::new(Arc::clone(&mock)) as Box<dyn Provider>,
            )],
            vec![],
            "model".into(),
        );

        let messages = vec![ChatMessage {
            tool_call_id: None,
            role: "user".to_string(),
            content: "use tools".to_string(),
        }];
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "shell",
                "description": "Run shell command",
                "parameters": {}
            }
        })];

        // chat_with_tools should delegate through the router to the mock.
        // MockProvider's default chat_with_tools calls chat_with_history -> chat_with_system.
        let result = router
            .chat_with_tools(&messages, &tools, "model", 0.7)
            .await
            .unwrap();
        assert_eq!(result.text.as_deref(), Some("tool-response"));
        assert_eq!(mock.call_count(), 1);
        assert_eq!(mock.last_model(), "model");
    }

    #[tokio::test]
    async fn chat_with_tools_routes_hint_correctly() {
        let (router, mocks) = make_router(
            vec![("fast", "fast-tool"), ("smart", "smart-tool")],
            vec![("reasoning", "smart", "claude-opus")],
        );

        let messages = vec![ChatMessage {
            tool_call_id: None,
            role: "user".to_string(),
            content: "reason about this".to_string(),
        }];
        let tools = vec![serde_json::json!({"type": "function", "function": {"name": "test"}})];

        let result = router
            .chat_with_tools(&messages, &tools, "hint:reasoning", 0.5)
            .await
            .unwrap();
        assert_eq!(result.text.as_deref(), Some("smart-tool"));
        assert_eq!(mocks[1].call_count(), 1);
        assert_eq!(mocks[1].last_model(), "claude-opus");
        assert_eq!(mocks[0].call_count(), 0);
    }

    struct FailThenOk {
        remaining_fails: std::sync::atomic::AtomicUsize,
        last_model: parking_lot::Mutex<String>,
    }

    #[async_trait]
    impl Provider for FailThenOk {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            *self.last_model.lock() = model.to_string();
            let left = self.remaining_fails.fetch_sub(1, Ordering::SeqCst);
            if left > 1 {
                anyhow::bail!("HTTP 410 (http_error): Gone end of life");
            }
            Ok("ok".into())
        }
    }

    #[async_trait]
    impl Provider for Arc<FailThenOk> {
        async fn chat_with_system(
            &self,
            system_prompt: Option<&str>,
            message: &str,
            model: &str,
            temperature: f64,
        ) -> anyhow::Result<String> {
            self.as_ref()
                .chat_with_system(system_prompt, message, model, temperature)
                .await
        }
    }

    #[tokio::test]
    async fn hint_peer_fallback_skips_retired_model() {
        let dead = FailThenOk {
            remaining_fails: AtomicUsize::new(2),
            last_model: parking_lot::Mutex::new(String::new()),
        };
        let live = MockProvider::new("live-ok");
        let dead = Arc::new(dead);
        let live = Arc::new(live);
        let router = RouterProvider::new(
            vec![
                (
                    "nvidia-dead".into(),
                    Box::new(Arc::clone(&dead)) as Box<dyn Provider>,
                ),
                (
                    "nvidia-live".into(),
                    Box::new(Arc::clone(&live)) as Box<dyn Provider>,
                ),
            ],
            vec![(
                "reasoning".into(),
                Route {
                    provider_name: "nvidia-dead".into(),
                    model: "dead-model".into(),
                    fallbacks: vec![("nvidia-live".into(), "live-model".into())],
                },
            )],
            "default".into(),
        )
        .with_hint_peer_fallback(true);

        let text = router
            .chat_with_system(None, "hi", "hint:reasoning", 0.2)
            .await
            .unwrap();
        assert_eq!(text, "live-ok");
        assert_eq!(live.last_model(), "live-model");
        assert_eq!(router.routed_model_label("hint:reasoning"), "live-model");
    }
}
