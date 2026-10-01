---
name: agent-loop
description: Run one iteration of the mmorpg multi-agent loop — review open agent PRs, promote drafted issues to ready specs, queue the next plan step, dispatch Sonnet tasks and report what Sol should pick up. Use when the user says "start/run the loop" or invokes /agent-loop; wrap in /loop for continuous pacing.
---

# Agent loop

You are the loop driver (Claude Opus). The contract for issues, labels and roles is `docs/AGENT_TASKS.md`; the rules every implementer follows are `AGENTS.md`. Read both at the start of every run, and `docs/STARTER_ZONE.md` before writing a new spec.

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
2. **Codex:** read the review comments and threads from `chatgpt-codex-connector` (`gh api repos/{owner}/{repo}/pulls/<n>/comments`, `.../reviews`, and the issue comments). Every finding must be fixed or answered in the thread. Check whether the review covers the head commit; if a substantial fix landed after it, comment `@codex review` and skip until it reports.
3. **Spec:** compare the diff with the issue's Decisions, Acceptance and Out of scope:
   - formats match exactly;
   - nothing out of scope slipped in;
   - acceptance tests exist;
   - native smoke was claimed where required;
   - the `browser-evidence` label is present where required.
   
   Also check the `AGENTS.md` invariants (authority boundaries, determinism, fail-closed versions).
4. **Verdict:**
   - **Ready:** `gh pr merge <n> --merge --delete-branch`. If auto mode denies the merge, do not work around it; list the PR as "ready for you to merge" in the report.
   - **Changes needed:** one PR comment with a numbered, concrete list. For a PR by Sonnet, dispatch Sonnet again with that list (step 4). For Sol, the report tells the user to send Sol back to the PR.

Never merge PRs in foundation repositories (3d-lab, game-server, physics-engine, …); list them for the user.

## 2. Promote drafts

For each `spec:draft` issue (often drafted in a ChatGPT chat):

- Check it against the current code on `origin/main`: versions, command tags, section names, module paths, budgets, open parallel tasks.
- Check it against `docs/AGENT_TASKS.md`: sizing, one format bump, the implementer label, every section present.
- If you can complete it by deciding things yourself, edit the body (`gh issue edit <n> --body-file …`), summarise what you changed in a comment, and swap `spec:draft` for `spec:ready`.
- If a decision belongs to the owner (scope, game design, anything touching authority or distribution), ask in a comment and label it `spec:needs-input`. Re-check those issues for answers on every run.

## 3. Keep each agent's queue at exactly one ready task

For each of `agent:sol` and `agent:sonnet` with no open `spec:ready` or `in-progress` task:

- Pick the next unfinished step: the open plan issues from `docs/STARTER_ZONE.md`, then the roadmap.
- Respect dependencies: a presentation task waits for its core task.
- Avoid conflicts: never queue two tasks that bump the same format version or edit the same HUD/module concurrently.
- Write the issue exactly per `docs/AGENT_TASKS.md` "Writing an issue", with labels `agent-task`, `spec:ready` and the `agent:*` label. Verify every number and name you cite against the code first.
- Link it from the parent plan issue with a one-line comment.

Write at most two new specs per run.

## 4. Dispatch

- **`agent:sonnet`** (ready, not in progress): add `in-progress`, then launch a background Agent:
  - `model: "sonnet"`, `isolation: "worktree"`;
  - prompt: "Implement issue #N of moritzbrantner/mmorpg. Read AGENTS.md, docs/AGENT_TASKS.md and the issue. Work on the branch the issue names, commit in small steps, run the focused checks plus whatever the issue lists that CI does not run, push, and open the PR with `Closes #N` only when the branch is complete (add the `browser-evidence` label for browser-visible changes). Report the PR URL and anything you could not verify."
  
  For a "changes needed" re-dispatch, give the PR number and the numbered list instead. Run at most one Sonnet task at a time.
- **`agent:opus`**: implement it yourself in a worktree following the same rules, or skip it this run if steps 1–3 already used the run.
- **`agent:sol`**: you cannot launch Sol. In the report, name the issue and give the user the line to paste: `Pick up #N per AGENTS.md and docs/AGENT_TASKS.md; one branch, one PR.`

## 5. Report

End with a compact table: each PR (merged / changes requested / waiting for CI or Codex / ready for the user to merge), each issue (promoted / needs input / newly queued / dispatched), and a "For you" list naming only the user's actions (merges auto mode refused, Sol hand-offs, questions).

## Pacing

- A single invocation does one run.
- For continuous operation the user runs `/loop /agent-loop`. Schedule the next wakeup around 1800 s while PRs wait on CI, Codex or Sol, and stop the loop when nothing is in flight and no plan steps remain.
- A finished background Sonnet agent re-invokes you; continue from step 1 for its PR.
