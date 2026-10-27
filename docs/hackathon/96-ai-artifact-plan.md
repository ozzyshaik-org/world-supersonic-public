# AI artifact plan

Status: plan only. Nothing is published by this document. No prompt, session transcript or planning export has been added to the repository, and no tool's history has been exported or reviewed for this plan.

## Why
ETHGlobal's submission details ([Use of AI Tools](https://ethglobal.com/events/mumbai/info/details)) say:

> Spec-Driven Development: Using spec-driven workflows (e.g., OpenSpec, Kiro, spec-kit) is permitted. If you use one, you must include all spec files, prompts, and planning artifacts in your submission repository. Judges need to see the full picture of how you directed the AI, not just the generated output.

The same page also asks for attribution ("Clearly document in your submission where and how AI tools were used in the project") and for meaningful human involvement. This project writes its specifications before the event and directs AI tools from them, so this plan treats the rule above as applying. It is to be rechecked against the official page before the final submission.

[AI_USAGE.md](../../AI_USAGE.md) records what was used and by whom. This plan covers how the prompts and planning artifacts themselves are kept and published.

## What counts as an artifact
- Spec files: the documents in `docs/hackathon/`, as committed.
- Planning artifacts: plans, task lists, design notes, review notes and decision records that directed or reviewed AI work.
- Prompts: the instructions given to an AI tool for event work, in any tool, including Cursor, Claude Code, Grok Bot, Kimi and Arc Studio.
- Tool outputs that changed the project and are not already visible as commits (for example the generated project in Arc Studio before export).

## Capture during the event
- [ ] Keep prompts and planning artifacts at the time they are used, not reconstructed afterwards. Save the prompt text, the tool, the date and time with its zone, and the file or feature it was for.
- [ ] Arc Studio prompts are saved the same way, including the prompt that starts the Studio project. Whether Arc Studio keeps or exports its own prompt history is to be rechecked at implementation.
- [ ] Keep the files in a private working location until they have been sanitized. A raw, unreviewed transcript is never committed.
- [ ] Record the date, tool and purpose in an index so that each artifact can be traced to the commit it influenced.

## Sanitizing before publication
- [ ] Remove or replace secrets and credentials (API keys, tokens, cookies, private keys, seed phrases, connection strings), private paths, private hostnames and IP addresses, email addresses and other personal data, and anything about live infrastructure.
- [ ] Mark each removal in the text with a visible placeholder such as `[REDACTED: secret]`. Do not delete without a mark.
- [ ] Do not edit the meaning, wording or order of a prompt. Only the sensitive part is replaced.
- [ ] Run a pattern scan over the files to be added (the same pattern families as in [AI_USAGE.md](../../AI_USAGE.md)), then read them once by hand.
- [ ] The owner approves each batch before it is committed.

## Redaction log
Every redaction is recorded here or in the index, with the artifact, the kind of content removed, the date and who removed it. Nothing has been redacted yet because nothing has been published.

| Artifact | What was removed (kind only) | Date | By |
|---|---|---|---|
| none yet | none yet | none yet | none yet |

## Unavailable history
Some history will not exist or cannot be published: tool sessions that were not saved, tools that do not export prompts, history from before this plan, and anything that contains secrets that cannot be safely redacted.
- [ ] Record each case in [AI_USAGE.md](../../AI_USAGE.md) with the tool, the period and the reason, in plain words ("not saved", "not exportable", "withheld: contains secrets").
- [ ] Never write a missing prompt from memory, from a summary or from the generated output and present it as the original. If a description of a missing prompt is useful, it is labelled as a later description, with its date and author, and is not placed in the prompt collection as a prompt.
- [ ] Private Cursor and Claude Code session history before the event stays private. [AI_USAGE.md](../../AI_USAGE.md) already says it is not published and is not a complete per-file authorship map.

## Human contribution
- [ ] Owner review and human work stay explicit in the record. For each batch, say which parts the owner wrote, which parts AI drafted and the owner edited, and which parts the owner only reviewed.
- [ ] Owner-review commits stay in history as they are. They are not squashed or rewritten.
- [ ] The record does not claim human work that did not happen, and it does not claim AI use that did not happen.

## Timing and location
- [ ] Event prompts and planning artifacts are captured as the event goes on and are published, sanitized, before the final submission, so that judges and the checklist in [95](95-submission-checklist.md) can point at them.
- [ ] Artifacts live in the repository under a dedicated directory chosen at that time. The directory name is not fixed by this plan.
- [ ] [AI_USAGE.md](../../AI_USAGE.md) lists them with their own scan, kept apart from the baseline rows, and does not backfill baseline rows.
- [ ] Pre-existing specs and planning documents that are already public are not republished.

## Not done by this document
This plan adds no prompts, no transcripts, no exports, no tags and no change to the baseline, tag or history rules in [CONTINUITY.md](../../CONTINUITY.md). Publishing is event work.
