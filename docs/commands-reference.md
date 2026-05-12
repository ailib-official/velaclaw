# VelaClaw Commands Reference

This reference is derived from the current CLI surface (`velaclaw --help`).

Last verified: **February 20, 2026**.

## Top-Level Commands

| Command | Purpose |
|---|---|
| `onboard` | Initialize workspace/config quickly or interactively |
| `agent` | Run interactive chat or single-message mode |
| `gateway` | Start webhook and WhatsApp HTTP gateway |
| `daemon` | Start supervised runtime (gateway + channels + optional heartbeat/scheduler) |
| `service` | Manage user-level OS service lifecycle |
| `doctor` | Run diagnostics and freshness checks |
| `status` | Print current configuration and system summary |
| `cron` | Manage scheduled tasks |
| `models` | Refresh provider model catalogs |
| `providers` | List provider IDs, aliases, and active provider |
| `channel` | Manage channels and channel health checks |
| `integrations` | Inspect integration details |
| `skills` | List/install/remove skills |
| `migrate` | Import from external runtimes (currently OpenClaw) |
| `config` | Export machine-readable config schema |
| `completions` | Generate shell completion scripts to stdout |
| `hardware` | Discover and introspect USB hardware |
| `peripheral` | Configure and flash peripherals |

## Command Groups

### `onboard`

- `velaclaw onboard`
- `velaclaw onboard --interactive`
- `velaclaw onboard --channels-only`
- `velaclaw onboard --force`
- `velaclaw onboard --api-key <KEY> --provider <ID> --memory <sqlite|lucid|markdown|none>`
- `velaclaw onboard --api-key <KEY> --provider <ID> --model <MODEL_ID> --memory <sqlite|lucid|markdown|none>`
- `velaclaw onboard --api-key <KEY> --provider <ID> --model <MODEL_ID> --memory <sqlite|lucid|markdown|none> --force`

`onboard` safety behavior:

- If `config.toml` already exists, `onboard` asks for explicit confirmation before overwrite.
- In non-interactive environments, existing `config.toml` causes a safe refusal unless `--force` is passed.
- Use `velaclaw onboard --channels-only` when you only need to rotate channel tokens/allowlists.

### `agent`

- `velaclaw agent`
- `velaclaw agent -m "Hello"`
- `velaclaw agent --provider <ID> --model <MODEL> --temperature <0.0-2.0>`
- `velaclaw agent --peripheral <board:path>`

### `gateway` / `daemon`

- `velaclaw gateway [--host <HOST>] [--port <PORT>]`
- `velaclaw daemon [--host <HOST>] [--port <PORT>]`

### `service`

- `velaclaw service install`
- `velaclaw service start`
- `velaclaw service stop`
- `velaclaw service restart`
- `velaclaw service status`
- `velaclaw service uninstall`

### `cron`

- `velaclaw cron list`
- `velaclaw cron add <expr> [--tz <IANA_TZ>] <command>`
- `velaclaw cron add-at <rfc3339_timestamp> <command>`
- `velaclaw cron add-every <every_ms> <command>`
- `velaclaw cron once <delay> <command>`
- `velaclaw cron remove <id>`
- `velaclaw cron pause <id>`
- `velaclaw cron resume <id>`

Notes:

- Mutating schedule/cron actions require `cron.enabled = true`.
- Shell command payloads for schedule creation (`create` / `add` / `once`) are validated by security command policy before job persistence.

### `models`

- `velaclaw models refresh`
- `velaclaw models refresh --provider <ID>`
- `velaclaw models refresh --force`

`models refresh` currently supports live catalog refresh for provider IDs: `openrouter`, `openai`, `anthropic`, `groq`, `mistral`, `deepseek`, `xai`, `together-ai`, `gemini`, `ollama`, `llamacpp`, `astrai`, `venice`, `fireworks`, `cohere`, `moonshot`, `glm`, `zai`, `qwen`, and `nvidia`.

### `channel`

- `velaclaw channel list`
- `velaclaw channel start`
- `velaclaw channel doctor`
- `velaclaw channel bind-telegram <IDENTITY>`
- `velaclaw channel add <type> <json>`
- `velaclaw channel remove <name>`

Runtime in-chat commands (Telegram/Discord while channel server is running):

- `/models`
- `/models <provider>`
- `/model`
- `/model <model-id>`

Channel runtime also watches `config.toml` and hot-applies updates to:
- `default_provider`
- `default_model`
- `default_temperature`
- `api_key` / `api_url` (for the default provider)
- `reliability.*` provider retry settings

`add/remove` currently route you back to managed setup/manual config paths (not full declarative mutators yet).

### `integrations`

- `velaclaw integrations info <name>`

### `skills`

- `velaclaw skills list`
- `velaclaw skills install <source>`
- `velaclaw skills remove <name>`

`<source>` accepts git remotes (`https://...`, `http://...`, `ssh://...`, and `git@host:owner/repo.git`) or a local filesystem path.

Skill manifests (`SKILL.toml`) support `prompts` and `[[tools]]`; both are injected into the agent system prompt at runtime, so the model can follow skill instructions without manually reading skill files.

### `migrate`

- `velaclaw migrate openclaw [--source <path>] [--dry-run]`

### `config`

- `velaclaw config schema`

`config schema` prints a JSON Schema (draft 2020-12) for the full `config.toml` contract to stdout.

### `completions`

- `velaclaw completions bash`
- `velaclaw completions fish`
- `velaclaw completions zsh`
- `velaclaw completions powershell`
- `velaclaw completions elvish`

`completions` is stdout-only by design so scripts can be sourced directly without log/warning contamination.

### `hardware`

- `velaclaw hardware discover`
- `velaclaw hardware introspect <path>`
- `velaclaw hardware info [--chip <chip_name>]`

### `peripheral`

- `velaclaw peripheral list`
- `velaclaw peripheral add <board> <path>`
- `velaclaw peripheral flash [--port <serial_port>]`
- `velaclaw peripheral setup-uno-q [--host <ip_or_host>]`
- `velaclaw peripheral flash-nucleo`

## Validation Tip

To verify docs against your current binary quickly:

```bash
velaclaw --help
velaclaw <command> --help
```
