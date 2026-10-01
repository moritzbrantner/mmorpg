---
name: agent-loop
description: Run one iteration of the mmorpg multi-agent loop — review open agent PRs, promote drafted issues to ready specs, queue the next plan steps, implement Opus tasks, dispatch Sonnet tasks and keep a backlog for Sol, which runs separately and occasionally. Use when the user says "start/run the loop" or invokes /agent-loop; wrap in /loop for continuous pacing.
---

# Agent loop

You are the loop driver (Claude Opus). The contract for issues, labels and roles is `docs/AGENT_TASKS.md`; the rules every implementer follows are `AGENTS.md`. Read both at the start of every run, and `docs/STARTER_ZONE.md` before writing a new spec.

**Sol is offline by default.** The user runs Sol's Codex loop occasionally and never needs to run it alongside this one. Never wait for Sol: keep the game moving with Opus and Sonnet, and treat `agent:sol` issues as a backlog Sol works through whenever it is started. If Sol does run at the same time, the `in-progress` label is the lock; never touch an `in-progress` Sol issue or push to a Sol branch.

One run = the steps below, in order, then a short report. Keep chat output to the report; put spec content into issues and review content into PR comments.

## 0. Baseline

- `git fetch` and work from `origin/main` (GIT-001). Never edit the user's checked-out branch; use a worktree for any change you make yourself.
- Make sure the labels in `docs/AGENT_TASKS.md` exist (`gh label create … || true`).
- Collect state:
  - `gh pr list --state open --json number,title,headRefName,author,labels,isDraft,url`
  - `gh issue list --label agent-task --state open --json number,title,labels,body`

## 1. Review open PRs

For each open, non-draft PR that closes an `agent-task` issue:

1. **CI:** `gh pr checks <n>`. If pending, skip it this run. If red, comment the failing check and log excerpt, then stop on this PR.
2. **Codex:** read the review comments and threads from `chatgpt-codex-connector` (`gh api repos/{owner}/{repo}/pulls/<n>/comments`, `.../reviews`, and the issue comments). Require a completed connector review covering the current head commit; the review-summary issue comment may record completion even when there are no findings. Skip this PR while that review is absent or running. Every finding must be fixed or answered in the thread. If the head changed after the completed review, comment `@codex review` when no current-head review is running and skip until it completes.
3. **Spec:** compare the diff with the issue's Decisions, Acceptance and Out of scope:
   - formats match exactly;
   - nothing out of scope slipped in;
   - acceptance tests exist;
   - native smoke was claimed where required;
   - the `browser-evidence` label is present where required.
   
   Also check the `AGENTS.md` invariants (authority boundaries, determinism, fail-closed versions).
4. **Verdict:**
   - **Ready:** `gh pr merge <n> --merge --delete-branch`. If auto mode denies the merge, do not work around it; list the PR as "ready for you to merge" in the report.
   - **Changes needed:** one PR comment with a numbered, concrete list. For a PR by Sonnet, dispatch Sonnet again with that list (step 4). For a PR by Opus, fix it yourself. For Sol, leave the comment; Sol's next run fixes its own PRs first.

Never merge PRs in foundation repositories (3d-lab, game-server, physics-engine, …); list them for the user.

## 2. Promote drafts

For each `spec:draft` issue (often drafted in a ChatGPT chat):

- Check it against the current code on `origin/main`: versions, command tags, section names, module paths, budgets, open parallel tasks.
- Check it against `docs/AGENT_TASKS.md`: sizing, one format bump, the implementer label, every section present.
- If you can complete it by deciding things yourself, edit the body (`gh issue edit <n> --body-file …`), summarise what you changed in a comment, and swap `spec:draft` for `spec:ready`.
- If a decision belongs to the owner (scope, game design, anything touching authority or distribution), ask in a comment and label it `spec:needs-input`. Re-check those issues for answers on every run.

## 3. Refresh and fill the queues

**Refresh the Sol backlog.** For each `agent:sol` + `spec:ready` issue not `in-progress`, re-check it against current `origin/main`: version numbers, tags, section names and "Parallel work". Edit the body when merges have moved them, with a one-line comment.

**Fill the queues.** A startable task is an open `spec:ready` issue whose "Start after" dependencies are merged.
- **Opus and Sonnet:** each keeps exactly one startable task.
- **Sol:** keeps up to three. Give Sol only work that nothing else will depend on soon. Examples: native parity, workloads, physics adoption slices, or independent core rules. Put critical-path core work (whatever the next presentation or plan step needs) on `agent:opus`.
- Never make an Opus or Sonnet task "Start after" an unstarted Sol task.
- If a Sol task already blocks queued work and has not been started for 24 hours, reassign it to `agent:opus`: swap the label and comment why.

For each agent below its target:

- Pick the next unfinished step: the open plan issues from `docs/STARTER_ZONE.md`, then the roadmap.
- Respect dependencies: a presentation task waits for its core task. Queue only a step whose own dependencies are already merged; if no such step exists for that agent, queue nothing and say so in the report.
- Avoid conflicts: never queue two tasks that bump the same format version or edit the same HUD/module concurrently, including Sol backlog tasks that may start at any time.
- Write the issue exactly per `docs/AGENT_TASKS.md` "Writing an issue", with labels `agent-task`, `spec:ready` and the `agent:*` label. Verify every number and name you cite against the code first.
- Link it from the parent plan issue with a one-line comment.

Write at most three new specs per run.

## 4. Dispatch

- **`agent:sonnet`** (ready, not in progress, and every "Start after" issue closed by a merged PR): add `in-progress`, then launch a background Agent:
  - `model: "sonnet"`, `isolation: "worktree"`;
  - prompt: "Implement issue #N of moritzbrantner/mmorpg. Read AGENTS.md, docs/AGENT_TASKS.md and the issue. Work on the branch the issue names, commit in small steps, run the focused checks plus whatever the issue lists that CI does not run, push, and open the PR with `Closes #N` only when the branch is complete (add the `browser-evidence` label for browser-visible changes). Report the PR URL and anything you could not verify."
  
  For a "changes needed" re-dispatch, give the PR number and the numbered list instead. Run at most one Sonnet task at a time.
- **`agent:opus`** (startable, not in progress): add `in-progress` and launch a background Agent so this loop keeps running:
  - `model: "opus"`, `isolation: "worktree"`;
  - the same prompt as for Sonnet.
  
  Run at most one Opus implementation at a time. For "changes needed" on an Opus PR, re-dispatch with the list.
- **`agent:sol`**: never dispatched from here. Sol runs the Codex `implementer-loop` skill (`.agents/skills/implementer-loop/`) whenever the user starts it. It fixes its own PRs first, then works through the backlog. Do not nag: mention the Sol backlog in the report only when it changed this run.

## 5. Report

End with a compact table: each PR (merged / changes requested / waiting for CI or Codex / ready for the user to merge), each issue (promoted / needs input / newly queued / dispatched), and a "For you" list naming only the user's actions (merges auto mode refused, questions, and the Sol backlog when it changed).

## Pacing

- A single invocation does one run.
- For continuous operation the user runs `/loop /agent-loop`. Schedule the next wakeup around 1800 s while PRs wait on CI or Codex.
- Stop the loop when no Opus or Sonnet work is in flight or startable and no plan steps remain for them. A non-empty Sol backlog alone is not a reason to keep looping.
- A finished background Sonnet or Opus agent re-invokes you; continue from step 1 for its PR.
