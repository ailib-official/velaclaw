# Workspace Prompt Templates (Draft README)

> **Status:** Structured draft for operators and packagers.  
> **Not yet** a shipped end-user guide — use this as the seed for a future User Guide chapter.  
> **Audience:** operators, home-lab maintainers, release packagers.  
> **Last updated:** 2026-07-29.

## 1. Summary

VelaClaw behavior is largely **document-driven**: short Markdown files in the agent workspace are injected into the system prompt. Accurate, small indexes steer the agent more effectively than large governance dumps or code changes for most “personality / habit / local context” issues.

| Layer | What it controls | Typical home |
|-------|------------------|--------------|
| **Runtime code** | Tools, SSH execution, parsers, security policy | VelaClaw binary |
| **`config.toml`** | Limits, allowlists, autonomy, providers | `~/.velaclaw*/config.toml` |
| **Workspace Markdown** | Persona, tool habits, host indexes, user prefs | `…/workspace/*.md` |

**Non-goals of this draft:** full onboard UX redesign, shipping a seed tarball in the installer, or changing the inject list in code (called out as follow-ups).

## 2. How injection works

On each agent session, VelaClaw loads workspace identity files into the system prompt (when present and non-empty), including:

| Injected today | Role |
|----------------|------|
| `AGENTS.md` | Operating posture (brevity, avoid meta-process talk) |
| `SOUL.md` | Persona, style, hard tool-format rules |
| `TOOLS.md` | Tool usage habits (SSH vs local path, GitHub/`gh`, etc.) |
| `IDENTITY.md` | Name / role labels |
| `USER.md` | User preferences + **short** environment index (recommended) |
| `HEARTBEAT.md` | Optional periodic-check notes |
| `BOOTSTRAP.md` | First-run / setup notes |
| `MEMORY.md` | Durable user facts (keep small) |

**Important:** files **not** in this list (for example a custom `INFRA.md`) are **not** auto-injected. Put critical facts in an injected file (usually `USER.md` / `TOOLS.md`), or teach the agent to `file_read` them — or later extend the inject list in code.

`velaclaw onboard` already seeds several of these files. Treat them as the product’s built-in seed; site-specific packs should **overlay** short indexes, not replace the whole SOUL with a novel.

## 3. Recommended template pack (layers)

Keep packs **small**. Prefer projection (short indexes) over importing whole repos (private planning repos, governance corpora, etc.).

```text
workspace/
  AGENTS.md      # thin posture
  SOUL.md        # persona + tool-call contract + “don’t re-paste tool output”
  TOOLS.md       # habits: SSH hosts, gh, optional-grep tips
  IDENTITY.md    # name/role
  USER.md        # prefs + compact host table (injected!)
  MEMORY.md      # durable facts only
  INFRA.md       # optional longer host notes (NOT injected unless listed above)
  _sources/      # optional read-only excerpts for humans / occasional file_read
```

### 3.1 Layer responsibilities

| Layer | Change frequency | Content rules |
|-------|------------------|---------------|
| Persona (`SOUL` / `AGENTS`) | Rare | Abstract rules; no secrets; no host IP laundry lists |
| Habits (`TOOLS`) | Occasional | Formats and “do / don’t”; examples must be complete `<tool_call>…</tool_call>` |
| User prefs (`USER`) | Per person | Language, brevity, “summarize tools, don’t duplicate” |
| Environment index (`USER` table and/or `INFRA`) | When infra changes | Host **aliases**, roles, canonical paths; **no** passwords/PATs |
| Config (`config.toml`) | Per install | `max_tool_iterations`, `max_actions_per_hour`, `allowed_commands`, autonomy |

### 3.2 Environment index (what belongs)

Include:

- SSH Host aliases (`piubt`, `lan-git`, `eos-hk`, …) and that they are **not** local folders
- Role one-liners (proxy / bare-git SoT / VPS)
- Canonical paths when non-obvious (e.g. bare repos under `/srv/git/repos`)
- “What this host is **not**” (e.g. LAN git SoT is bare git + SSH, not Gitea)

Exclude:

- Passwords, PATs, private keys, token-extraction recipes
- Full copies of private planning repos or governance corpora
- Long risk essays or speculative architecture

## 4. Writing guidelines (quality bar)

1. **Short beats complete.** Prompt budget is finite; indexes of tens of lines beat hundreds.
2. **Abstract rules in SOUL; concrete hosts in USER/INFRA.** Don’t encode one-off e2e scripts into persona.
3. **Tool calls must be complete.** Bare JSON or a lone closing `</tool_call>` is shown as chat and **does not execute**.
4. **Don’t re-paste tool UI.** After `── tool:… ──`, the assistant should summarize only.
5. **Empty `Error:`** often means exit ≠ 0 with empty stderr (e.g. `grep` no match). Prefer existence checks or `|| true` for optional greps.
6. **Restart the agent** (or new session) after editing workspace docs or `config.toml` so CLI reloads them.

## 5. Config knobs that docs cannot replace

Workspace Markdown does **not** raise rate limits. Set these in `config.toml` (example magnitudes used in a home-lab trial):

| Key | Role | Notes |
|-----|------|--------|
| `[agent].max_tool_iterations` | Tool loop depth per turn | Default is low (~10); raise for multi-step ops |
| `[autonomy].max_actions_per_hour` | Action budget | Default is low (~20) |
| `[autonomy].allowed_commands` | Shell allowlist | `full` autonomy also merges extras; still add `gh` / `jq` if needed |
| `[autonomy].level` | Autonomy posture | Affects approval and allowlist merge |

Keep secrets in the OS keychain / encrypted config / SSH agent — never in Markdown templates.

## 6. Suggested seed layout for packaging (later)

This repository already includes an **example** pack you can copy manually:

- [`seeds/README.md`](../seeds/README.md) — pack index
- [`seeds/home-lab-lan/`](../seeds/home-lab-lan/) — LAN / home-lab overlay (customize hosts before use)

Future release packaging can treat this README as the contract for additional packs:

```text
seeds/
  default/           # onboard-compatible persona stubs (future)
  home-lab-lan/      # example overlay: USER host table + TOOLS SSH/gh habits
  README.md
  README.zh-CN.md
```

Install merge policy (proposal only):

1. Write missing files; do not overwrite customized `SOUL.md` / `USER.md` without `--force` / confirmation.
2. Overlays may append an “Local infrastructure (index)” section to `USER.md` if absent.
3. Never ship secrets inside seeds.
4. Optional: `velaclaw onboard --seed <name>` (not implemented yet).

## 7. Verification checklist

After applying a pack:

- [ ] Ask a vague remote question using an SSH alias (host must not be treated as a local directory).
- [ ] Confirm tool results appear under `── tool:shell ──` (not only as pasted JSON).
- [ ] Confirm the final `>>` reply is a short summary, not a second full dump.
- [ ] Confirm `config.toml` limits match the intended install profile.
- [ ] Confirm no passwords/PATs appear under `workspace/`.

## 8. Path to a User Guide chapter

When promoting this draft:

1. Move narrative into `docs/user-guide/` with screenshots of onboard + workspace files.
2. Keep this file (or a slim reference) as the **template contract** for packagers.
3. Optionally add `INFRA.md` to the runtime inject list if many installs need a dedicated host index.
4. Wire seed packs into `velaclaw onboard` / bootstrap as an explicit `--seed <name>` option.

## 9. Related docs

- Getting started hub: [getting-started/README.md](getting-started/README.md)
- Config reference: [config-reference.md](config-reference.md)
- Policy / approval: [policy-approval-reference.md](policy-approval-reference.md)
- Commands: [commands-reference.md](commands-reference.md)

---

**Document type:** draft README / packaging contract  
**Next promotion target:** User Guide — “Workspace persona & local indexes”
