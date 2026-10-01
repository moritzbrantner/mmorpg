---
name: Agent task
about: One PR-sized task for a coding agent (see docs/AGENT_TASKS.md)
title: "<Area> <step>: <what the player or system gains>"
labels: ["agent-task", "spec:draft"]
---

Step <n> of `docs/STARTER_ZONE.md` (part of #<parent>). Intended implementer: **<Opus|Sol|Sonnet>**. Start after: <#N or "nothing">. One branch (`agent/<topic>`), one PR; follows the `AGENTS.md` **Execution scope** rules.

## Goal

<Two or three sentences: what the player or system can do afterwards.>

## Decisions already made (do not reopen)

- **Rules and numbers:** <…>
- **Formats:** <command tags/fields, snapshot sections, version bumps, budget; or "no format change">
- **Compatibility:** <what happens to existing state, old snapshots and existing scenarios>
- **Left to the implementer:** <explicitly delegated choices, recorded in the PR>

## Acceptance

- <tests and scenarios>
- <`scripts/smoke-native.py` / `browser-evidence` label when relevant>
- CI green and every Codex review finding addressed or answered.

## Expected changes

- <crates/files/docs>

## Out of scope

- <…>
- Foundation, tooling, pin-refresh and budget work.

## Parallel work

- <open tasks touching the same files, or "none">
