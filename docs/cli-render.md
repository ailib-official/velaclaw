# CLI Terminal Render

How VelaClaw formats agent output in the terminal (Markdown → ANSI/box-drawing, CJK width, long-output fold).

Last verified: **July 9, 2026** (VL-CLI-RENDER-001..004).

## What it does

- Renders common Markdown (headings, emphasis, lists, tables, fenced code, links) for TTY sessions
- Computes display width with `unicode-width` so CJK/emoji alignment stays correct
- Folds long tool/code blocks in interactive REPL and offers `/expand <id>`
- Stays pipe/CI friendly: non-TTY and `NO_COLOR` strip ANSI; box-drawing characters remain

IM channels (Telegram/Discord/…) are unchanged — they keep platform-native formatting.

## Quick usage

```bash
velaclaw agent                 # interactive: Markdown + fold when TTY
velaclaw agent -m "Hello"      # one-shot: render, no fold
velaclaw agent --no-color      # plain text (no ANSI / no Markdown structure)
velaclaw agent --no-fold       # interactive but never fold long blocks
```

In the REPL:

```text
/expand 1    # replay folded payload for id 1
/help        # lists /expand among other commands
```

## Config

Optional `[cli_render]` in `config.toml` (see [config-reference.md](config-reference.md)):

```toml
[cli_render]
fold_lines = 10
markdown_enabled = true
```

Omit the section to keep the same defaults.

## Related

- Commands: [commands-reference.md](commands-reference.md) (`agent` flags)
- Config keys: [config-reference.md](config-reference.md) (`[cli_render]`)
- Chat UX notes: [user-guide/03-chat-with-ai.md](user-guide/03-chat-with-ai.md)
