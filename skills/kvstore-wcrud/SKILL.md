---
name: kvstore-wcrud
description: >
  Use this skill when working on this repository and you need to read/write/update/delete
  persistent project knowledge in kvstore (records, tags, and markdown files), especially
  for summaries, decisions, TODOs, and Codex handoff notes across namespaces.
---

# kvstore wCRUD Skill

Use this skill for persistent knowledge operations in this project.

## When To Use

Use this skill when the user asks to:
- save a conclusion, summary, decision, or handoff note
- keep project memory across sessions
- organize notes by namespace/tag
- create/update/delete kvstore records or tags
- store full markdown files in kvstore (`put-file` / `get-file`)

Typical trigger phrases:
- "save this in kv"
- "write a conclusion"
- "store this summary"
- "update project memory"
- "add tag / rename tag / delete tag"

## When To Write

Write to kvstore when information is:
- durable: useful after this session
- actionable: decision, plan, risk, TODO, assumption, outcome
- user-facing memory: something the user expects to retrieve later

Do NOT write when information is:
- ephemeral command output
- redundant with existing key content (without meaningful change)
- sensitive unless the user explicitly asked to store it

## Namespace Rules

1. Prefer explicit namespace from user request (`-n <name>`).
2. If not provided, use current default behavior (`default` or `KVSTORE_NAMESPACE`).
3. Keep all related writes in the same namespace for one task.

## Key and Tag Conventions

- Use stable, descriptive snake_case keys.
- Prefer one topic per key.
- Use tags for retrieval facets (`@summary`, `@decision`, `@todo`, `@risk`, `@codex`).
- For periodic updates, keep the same key and update content.

## CRUD Workflows

### Create or Update Record

```bash
kv -n <ns> add <key> "<value>" @tag1 @tag2
```

### Read Record

```bash
kv -n <ns> get <key>
kv -n <ns> search <pattern> --limit 20
kv -n <ns> list
```

### Delete Record

```bash
kv -n <ns> remove <key>
```

### Tag CRUD (Live UI / API mode)

If `kv serve` is running, prefer UI actions for:
- add/remove tag per record
- rename/delete tag globally

If only CLI is available, perform record updates with normalized tags via `add`.

## Markdown File Workflows

Use these for long structured notes:

### Store markdown file content

```bash
kv -n <ns> put-file <key> ./path/to/file.md @summary @codex
```

### Restore markdown file content

```bash
kv -n <ns> get-file <key> ./path/to/output.md
```

## Recommended Write Moments

After completing meaningful work, store:
1. `project_summary` (what changed)
2. `next_steps` (ordered actionable items)
3. `known_risks` (open issues/assumptions)

Suggested tags:
- `@summary @codex @handoff`
- `@todo @priority`
- `@risk`

## Quality Checklist Before Writing

- namespace is correct
- key name is stable and specific
- content is concise and not duplicated
- tags improve discoverability
- write succeeded (no silent failure)
