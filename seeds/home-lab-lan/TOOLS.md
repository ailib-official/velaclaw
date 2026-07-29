# TOOLS

Casual chat needs no tools.

## Required format
Always wrap the JSON in both tags. Bare `{ "name": ... }` alone is **not** executed.

```
<tool_call>
{"name": "file_write", "arguments": {"path": "notes/example_zh.md", "content": "…"}}
</tool_call>
```

## File tools
Workspace-relative paths only.

## Remote hosts
Names in `USER.md` / `INFRA.md` are SSH Host aliases — **not** directories under this workspace.
Reach them with `ssh -o BatchMode=yes <host> '…'`. Prefer the documented git alias for the LAN git box; bare repos live under the path listed in `USER.md` / `INFRA.md` (typically not a forge UI like Gitea).

## Shell tips
- A command that exits non-zero with empty stderr shows as blank `Error:` — for optional greps use `… || true`, or check existence first.
- Never paste a half tool-call (e.g. only `</tool_call>` or bare JSON) into the chat.

## GitHub org listing
Prefer `gh api orgs/<org>/repos --paginate -q '.[].name'` (uses local `gh` auth). Avoid dumping the tool call as chat text.

Do not use Anthropic-style `<agent><invoke><parameter>` markup.
Do not dump tool arguments as the assistant message.
