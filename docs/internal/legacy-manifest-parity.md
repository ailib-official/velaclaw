# Legacy factory ↔ ai-protocol parity (ZS-ML-014)

**Purpose.** Before **`legacy-providers` is removed** (ZS-ML-015), every legacy-only string key/resolution arm in `zerospider` must have an equivalent **`provider/model` + ai-protocol manifest** path (canonical vendor manifests live in [`ailib-official/ai-protocol`](https://github.com/ailib-official/ai-protocol)).

**Rule.** ZeroSpider must not introduce new vendor-special cases off-manifest; gaps are fixed in ai-protocol manifests or explicitly marked unsupported.

## Coverage table

| Legacy trigger / alias family | Equivalent protocol `provider_id` (example) | Notes |
|------------------------------|-----------------------------------------------|-------|
| `openrouter` | `openrouter` | Use manifests under ai-protocol (`v2/providers/openrouter.yaml` when present); BYOK keys follow manifest `auth`/env mapping. |
| `anthropic` | `anthropic` | V2 manifests + model registry IDs. |
| `openai` | `openai` | V2 manifests (fixture + upstream `v2/providers/openai.yaml`). |
| `gemini`, `google`, `google-gemini` | `google` | ai-protocol naming uses `google` as provider id for Gemini manifests. |
| `ollama` | `ollama` | Manifest-driven endpoints; legacy used local/default base URL overrides via `api_url`. |
| `bedrock`, `aws-bedrock` | `amazon_bedrock` or registry id matching ai-protocol (`bedrock`-named manifests per repo) | Align `provider/model` IDs with **`models` registries** in ai-protocol. |
| `copilot`, `github-copilot` | *TBD upstream manifest id* | If no published manifest parity, treat as **unsupported** protocol path until ai-protocol publishes one — track issue on ai-protocol repo. |
| OpenAI-compat shorthands (`groq`, `mistral`, `deepseek`, `together`, …) | same string as `provider_id` where manifests exist (`groq.yaml`, …) | For `custom:http(s)…` URLs, parity is **bring-your-own-protocol** manifest (no special legacy URL arm). |
| `custom:http(s)…` | Custom provider manifests / team-owned YAML in protocol tree | Not required to live in upstream ai-protocol; parity is “protocol-can-express-this-endpoint”. |
| `anthropic-custom:http(s)…` | Anthropic-compat manifest abstraction | Prefer protocol manifest with Anthropic-compatible transport; unsupported until manifests exist → document + error link (ZS-ML-015). |

**Verification**

- Automated: integration test **`protocol_fixture_resolves_openai_without_legacy`** (see `tests/protocol_manifest_parity.rs`) sets `AI_PROTOCOL_DIR` to the checked-in minimal fixture (`tests/fixtures/ai-protocol-min`) and asserts `create_provider(\"openai/gpt-5.2\", …)` succeeds **without enabling `legacy-providers`**.
- Human: maintainer acknowledgment that rows marked *TBD* are either migrated or consciously **unsupported**.

**Related plans**

- `ai-lib-plans`: `ZS-ML-015` removes `legacy-providers`; `ZS-ML-016` updates user-facing deprecation/migration wording.
