# AI usage

Status: pre-start documentation, based on facts confirmed by the owner. Nothing in this file is guessed. No percentages and
no per-file claims are made here.

This file covers the baseline (everything up to the annotated tag `baseline-pre-ethglobal-mumbai` and the pre-kickoff
documentation merged before the event). Baseline assistance and event assistance will be separated once the event begins:
event work is everything after the annotated tag `ethglobal-mumbai-2026-start`, and its AI usage will be recorded separately
from this baseline section. See [CONTINUITY.md](CONTINUITY.md) for the baseline/event split.

## Confirmed facts

- AI tools used across the project: Cursor, Claude Code, Grok Bot and ChatGPT/Codex. This file does not map each tool to an area.
- Areas AI assisted: backend and indexer code; documentation; tests and CI; architecture and specifications. For architecture and
  specifications, AI assisted the drafting; the owner designed the architecture and made the decisions.
- Tracking of history: exact per-file AI history is recorded in git commit messages and in Cursor and Claude Code session
  history. The session history is private and is not published. This file does not state per-file proportions.
- Owner (hackamelon / [@ozzyshaik](https://github.com/ozzyshaik)): designed the architecture; made operational decisions and
  operated the stack; reviewed AI-written code and documentation; ran tests and verified results; and wrote some code by hand.
- Teammate ([@nasiparveen006](https://github.com/nasiparveen006)): owns business development and product: pricing,
  documentation and positioning, partner outreach, and the demo and submission video.
- The pre-kickoff specifications and documentation merged in PR #3 and PR #4 were drafted with AI assistance and reviewed
  and edited by the owner. The commit history of those PRs carries owner-review commits (for example "docs: apply owner review: ...").
- PR #5 itself (this documentation change) was drafted with the help of an AI assistant, Grok Bot, the owner's assistant, and is reviewed by the owner before merge.

## Usage by area

| Area | AI tools used | AI contribution | Human contribution and review | Supporting artifacts |
|---|---|---|---|---|
| Architecture/specifications | Cursor, Claude Code, Grok Bot, ChatGPT/Codex (across the project; not mapped per area) | AI assisted drafting, including the pre-kickoff specs in `docs/hackathon/` (PR #3, PR #4) | The owner designed the architecture and decided; the owner reviewed and edited the pre-kickoff specs (PR #3, PR #4) | `docs/hackathon/01-x402-arc-gateway.md`, `docs/hackathon/02-world-id-gating.md`, `docs/hackathon/03-ens-integration.md`. Git commit messages. Sanitized specs and prompts: None published yet; sanitized artifacts may be added after review for secrets. |
| Backend/indexer code | Cursor, Claude Code, Grok Bot, ChatGPT/Codex (across the project; not mapped per area) | AI assisted backend and indexer code. The exact per-file proportion was not tracked in this file; per-file history is in git commit messages and in private Cursor and Claude Code session history | The owner designed the architecture, made operational decisions and operated the stack, reviewed AI-written code, ran tests and verified results, and wrote some code by hand | Git commit messages. Private Cursor and Claude Code session history (not published). Sanitized artifacts: None published yet; sanitized artifacts may be added after review for secrets. |
| Documentation | Cursor, Claude Code, Grok Bot, ChatGPT/Codex (across the project; not mapped per area) | AI assisted documentation, including PR #3, PR #4 and PR #5 | The owner reviewed AI-written documentation and edited the PR #3 and PR #4 documentation. The teammate owns pricing, documentation and positioning on the business side | Git commit messages. Sanitized artifacts: None published yet; sanitized artifacts may be added after review for secrets. |
| Tests and CI | Cursor, Claude Code, Grok Bot, ChatGPT/Codex (across the project; not mapped per area) | AI assisted tests and CI. CI currently only builds and vets the Go services; it does not run tests | The owner ran tests and verified results | Git commit messages. Sanitized test plans: None published yet; sanitized artifacts may be added after review for secrets. |
| UI/media/assets | Not yet determined; will be confirmed before submission | Not yet determined; will be confirmed before submission | The teammate owns the demo and submission video. Review of any generated assets: not yet determined; will be confirmed before submission | Not yet determined; will be confirmed before submission |

## Published AI artifacts

None published yet; sanitized artifacts may be added after review for secrets.

## Baseline versus event assistance

Baseline assistance (this file, as written before the event) and event assistance will be recorded separately once the event
begins. Event entries will be added after the tag `ethglobal-mumbai-2026-start` exists and will not be backfilled into the
baseline rows above.
