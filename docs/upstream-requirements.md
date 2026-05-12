# Upstream Requirements Summary

## Overview

During the integration of ai-lib-rust into VelaClaw, several gaps and enhancement opportunities were identified. This document summarizes the new requirements for ai-lib-rust and ai-protocol projects.

---

## 1. ai-lib-rust Requirements

### 1.1 API Surface Improvements

#### Issue: ChatRequestBuilder lacks `model()` method

**Current State**:
```rust
// ChatRequestBuilder doesn't have a model() method
let response = client.chat()
    .messages(messages)
    .temperature(0.7)
    .execute()  // model must be set during client creation
    .await?;
```

**Requirement**: Add `model()` method to `ChatRequestBuilder` for per-request model override:

```rust
let response = client.chat()
    .messages(messages)
    .model("gpt-4o")  // NEW: override model per request
    .temperature(0.7)
    .execute()
    .await?;
```

**Benefit**: Enables single-client multi-model usage patterns.

---

#### Issue: AiClient not Clone-able

**Current State**:
```rust
pub struct AiClient { ... }  // No Clone implementation
```

**Requirement**: Implement `Clone` for `AiClient`:

```rust
#[derive(Clone)]
pub struct AiClient { ... }
```

**Use Case**: Sharing client across async tasks, streaming contexts.

---

#### Issue: ToolDefinition vs generic JSON tools

**Current State**:
```rust
pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self;
```

**Requirement**: Add overload for generic JSON tools:

```rust
pub fn tools_json(mut self, tools: Vec<serde_json::Value>) -> Self;
```

**Benefit**: Easier integration with existing tool systems that use JSON Schema.

---

### 1.2 New Capabilities

#### Request: Provider Metrics API

**Requirement**: Expose internal metrics for external scoring systems:

```rust
impl AiClient {
    /// Get provider performance metrics
    pub fn metrics(&self) -> ProviderMetrics {
        ProviderMetrics {
            total_requests: self.stats.total_requests,
            successful_requests: self.stats.successful_requests,
            avg_latency_ms: self.stats.avg_latency_ms,
            total_tokens: self.stats.total_tokens,
            total_cost: self.stats.total_cost,
        }
    }
}
```

**Use Case**: VelaClaw's ProviderScorer needs real-time metrics for adaptive model selection.

---

#### Request: Multi-Model Client Pool

**Requirement**: Manage multiple providers from a single interface:

```rust
pub struct MultiModelClient {
    clients: HashMap<String, AiClient>,
}

impl MultiModelClient {
    pub fn new(provider_configs: Vec<ProviderConfig>) -> Result<Self>;
    
    /// Get client for specific provider/model
    pub fn client(&self, model_id: &str) -> Option<&AiClient>;
    
    /// Get best client based on criteria
    pub fn select_best(&self, criteria: SelectionCriteria) -> Option<&AiClient>;
}
```

**Use Case**: Simplifies multi-provider scenarios in VelaClaw.

---

#### Request: Negotiation Helper

**Requirement**: Built-in support for multi-model response aggregation:

```rust
pub struct Negotiator {
    strategy: NegotiationStrategy,
}

impl Negotiator {
    pub fn negotiate(&self, responses: Vec<UnifiedResponse>) -> NegotiationResult;
}

pub enum NegotiationStrategy {
    Voting,
    Consensus,
    BestOfN,
    SelfConsistency,
}
```

**Use Case**: VelaClaw's agent loop can leverage built-in negotiation.

---

### 1.3 Error Handling Enhancements

#### Request: Structured Error Classification

**Current State**: Errors are generic `Error` enum.

**Requirement**: Add structured error types for better classification:

```rust
pub enum ProviderError {
    RateLimited {
        retry_after: Option<Duration>,
        is_business_error: bool,  // quota exceeded, plan restriction
    },
    AuthFailed { reason: String },
    ModelNotFound { model: String },
    ContextWindowExceeded { tokens: u64, limit: u64 },
    Timeout { duration: Duration },
    Network { source: reqwest::Error },
}

impl Error {
    pub fn as_provider_error(&self) -> Option<&ProviderError>;
    pub fn is_retryable(&self) -> bool;
    pub fn retry_after(&self) -> Option<Duration>;
}
```

**Use Case**: VelaClaw's ReliableProvider can make smarter retry decisions.

---

### 1.4 Streaming Improvements

#### Request: Controlled Stream with Cancel Handle

**Current State**: `execute_stream()` returns a stream without cancellation.

**Requirement**: Expose cancel handle in streaming API:

```rust
// Already exists but needs better documentation
pub async fn execute_stream_with_cancel(
    self,
) -> Result<(Pin<Box<dyn Stream<Item = Result<StreamingEvent>>>>, CancelHandle)>;
```

**Use Case**: User-initiated cancellation in interactive sessions.

---

## 2. ai-protocol Requirements

### 2.1 Scoring Metadata

#### Request: Provider Quality Metrics

**Requirement**: Add optional quality/reliability metadata to provider YAML:

```yaml
id: openai
name: OpenAI
# ... existing fields ...

# NEW: Scoring metadata
scoring:
  baseline_latency_ms: 800
  baseline_cost_per_1k_input: 0.005
  baseline_cost_per_1k_output: 0.015
  reliability_score: 0.99
  quality_score: 0.92
  last_updated: "2024-01-15"
```

**Use Case**: Enables pre-scoring providers before any requests are made.

---

### 2.2 Model Registry Enhancement

#### Request: Extended Model Profiles

**Requirement**: Add task-specific metadata to model definitions:

```yaml
# v1/models/gpt-4o.yaml
id: gpt-4o
provider: openai
context_window: 128000

# NEW: Task-specific capabilities
capabilities:
  tasks:
    - coding
    - reasoning
    - creative
    - tool_use
    - vision
  supports_parallel_tool_calls: true
  supports_structured_output: true
  supports_vision: true
  
# NEW: Performance hints
performance:
  avg_latency_ms: 800
  throughput_tokens_per_sec: 50
  
# NEW: Cost information
pricing:
  input_per_1k: 0.005
  output_per_1k: 0.015
  currency: USD
```

**Use Case**: Enables smarter model selection without hardcoding profiles.

---

### 2.3 Negotiation Patterns

#### Request: Standardized Response Comparison

**Requirement**: Define response equivalence for voting/consensus:

```yaml
# In provider YAML or separate schema
negotiation:
  # How to compare responses for equality
  equivalence:
    # Normalize whitespace
    normalize_whitespace: true
    # Compare semantic meaning (embeddings)
    semantic_threshold: 0.95
    # Extract key answers for comparison
    answer_extraction_pattern: "the answer is (.+)"
```

**Use Case**: Improves accuracy of voting-based negotiation.

---

### 2.4 Error Classification Rules

#### Request: Provider-Specific Error Mapping

**Requirement**: Define error classification in protocol:

```yaml
errors:
  mappings:
    - pattern: "rate limit exceeded"
      classification: rate_limited
      retryable: true
      extract_retry_after: "retry after (\\d+)"
      
    - pattern: "insufficient quota"
      classification: rate_limited
      retryable: false
      is_business_error: true
      
    - pattern: "context length exceeded"
      classification: context_exceeded
      retryable: false
      
    - pattern: "invalid api key"
      classification: auth_failed
      retryable: false
```

**Use Case**: Standardizes error handling across all providers.

---

### 2.5 Health Check Endpoints

#### Request: Provider Health Configuration

**Requirement**: Define how to check provider health:

```yaml
health:
  check_endpoint: "/models"
  method: GET
  expected_status: 200
  timeout_ms: 5000
  interval_ms: 30000
  
  # Fallback check if primary fails
  fallback:
    method: POST
    endpoint: "/chat/completions"
    body:
      model: "${model_id}"
      messages: [{"role": "user", "content": "ping"}]
      max_tokens: 1
```

**Use Case**: Remote deployment health monitoring.

---

## 3. Integration Recommendations

### 3.1 Feature Flag Coordination

Suggest adding a feature matrix document to ai-protocol:

```
| Feature          | ai-lib-rust | VelaClaw | Notes                    |
|------------------|-------------|----------|--------------------------|
| embeddings       | ✅          | ❌       | Requires feature flag    |
| batch            | ✅          | ✅       | Integrated               |
| negotiation      | ❌          | ✅       | VelaClaw implementation  |
| scoring          | ❌          | ✅       | VelaClaw implementation  |
| multi-model      | ❌          | ✅       | VelaClaw implementation  |
```

### 3.2 Shared Test Fixtures

Recommend creating shared test protocol files in ai-protocol for:
- Mock provider responses
- Error scenarios
- Edge cases (empty responses, malformed JSON)

### 3.3 Version Compatibility

Document minimum required versions:

```
ai-protocol >= v1.2.0 (for scoring metadata)
ai-lib-rust >= v0.9.0 (for model() override)
```

---

## 4. Priority Summary

### High Priority

| Project | Requirement | Impact |
|---------|-------------|--------|
| ai-lib-rust | `model()` method on ChatRequestBuilder | Critical for multi-model |
| ai-lib-rust | Clone for AiClient | Required for streaming |
| ai-protocol | Error classification rules | Improves reliability |

### Medium Priority

| Project | Requirement | Impact |
|---------|-------------|--------|
| ai-lib-rust | Provider metrics API | Enables smart routing |
| ai-protocol | Model task capabilities | Better model selection |
| ai-protocol | Pricing metadata | Cost-aware routing |

### Low Priority

| Project | Requirement | Impact |
|---------|-------------|--------|
| ai-lib-rust | Negotiation helper | Convenience feature |
| ai-protocol | Health check config | Deployment automation |

---

## 5. Proposed Contributions

The VelaClaw team can contribute back:

1. **Scoring System**: The `ProviderScorer` implementation could be generalized and moved to ai-lib-rust
2. **Selector Logic**: `AdaptiveSelector` patterns could inform ai-lib-rust's model selection
3. **Negotiation Strategies**: Battle-tested negotiation patterns from production use
4. **Error Classification**: Comprehensive error mapping from 28+ providers

---

*Document generated from VelaClaw feat/ai-protocol-integration branch*
*Date: 2026-02-20*
