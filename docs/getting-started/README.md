# Getting Started Docs

For first-time setup and quick orientation.

## Start Path

1. Main overview and quick start: [../../README.md](../../README.md)
2. One-click setup and dual bootstrap mode: [../one-click-bootstrap.md](../one-click-bootstrap.md)
3. Find commands by tasks: [../commands-reference.md](../commands-reference.md)

## Choose Your Path

| Scenario | Command |
|----------|---------|
| I have an API key, want fastest setup | `velaclaw onboard --api-key sk-... --provider openai/gpt-5.2` |
| I want guided prompts | `velaclaw onboard --interactive` |
| Config exists, just fix channels | `velaclaw onboard --channels-only` |
| Config exists, I intentionally want full overwrite | `velaclaw onboard --force` |
| Using subscription auth | See [Subscription Auth](../../README.md#subscription-auth-openai-codex--claude-code) |

## Onboarding and Validation

- Quick onboarding: `velaclaw onboard --api-key "sk-..." --provider openai/gpt-5.2`
- Interactive onboarding: `velaclaw onboard --interactive`
- Existing config protection: reruns require explicit confirmation (or `--force` in non-interactive flows)
- Ollama cloud models (`:cloud`) require a remote `api_url` and API key (for example `api_url = "https://ollama.com"`).
- Validate environment: `velaclaw status` + `velaclaw doctor`

## Workspace persona & local indexes

Draft contract for Markdown templates that steer the agent (persona, tool habits, host indexes) without code changes:

- English: [../workspace-prompt-templates.md](../workspace-prompt-templates.md)
- 简体中文: [../workspace-prompt-templates.zh-CN.md](../workspace-prompt-templates.zh-CN.md)
- Example seed pack: [../../seeds/home-lab-lan/](../../seeds/home-lab-lan/)

## Next

- Runtime operations: [../operations/README.md](../operations/README.md)
- Reference catalogs: [../reference/README.md](../reference/README.md)
