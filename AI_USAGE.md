# AI usage

Status: pre-start documentation, based on facts confirmed by the owner. Nothing in this file is guessed. No percentages and
no per-file claims are made here.

This file covers the baseline (everything up to the annotated tag `baseline-pre-ethglobal-mumbai` and the pre-kickoff
documentation merged before the event). Baseline assistance and event assistance will be separated once the event begins:
event work is everything after the annotated tag `ethglobal-mumbai-2026-start`, and its AI usage will be recorded separately
from this baseline section. See [CONTINUITY.md](CONTINUITY.md) for the baseline/event split.

## Confirmed facts

- AI tools used across the project: Cursor, Claude Code, Grok Bot and Kimi. This file does not map each tool to an area.
- Kimi's role: Kimi assisted repository review, continuity and specification review, and documentation drafting/revision. Recommendations and wording from these reviews were incorporated into the pre-kickoff documentation. Kimi did not directly modify the repository or operate the infrastructure.
- Areas AI assisted: backend and indexer code; documentation; tests and CI; architecture and specifications. For architecture and
  specifications, AI assisted the drafting; the owner designed the architecture and made the decisions.
- Tracking of history: Public Git history records commits and file changes. Private Cursor and Claude Code session history provides additional provenance where available. Together they do not constitute a complete per-file AI/authorship map, and exact per-file AI proportions were not tracked.
- Owner (hackamelon / [@ozzyshaik](https://github.com/ozzyshaik)): designed the architecture; made operational decisions and
  operated the stack; reviewed AI-written code and documentation; ran tests and verified results; and wrote some code by hand.
- Teammate ([@nasiparveen006](https://github.com/nasiparveen006)): owns business development and product: pricing,
  documentation and positioning, partner outreach, and the demo and submission video.
- The pre-kickoff specifications and documentation merged in PR #3 and PR #4 were drafted with AI assistance and reviewed
  and edited by the owner. The commit history of those PRs carries owner-review commits (for example "docs: apply owner review: ...").
- `docs/hackathon/04-agent-pass.md`, `docs/hackathon/90-stretch-options.md` and the accompanying README, CONTINUITY (committed-scope list, stretch-options paragraph and partner-prize table only), EVIDENCE, DEMO_PLAN and scope-order edits (the pre-kickoff Agent Pass documentation PR, including its later revisions) were drafted with AI assistance (Grok Bot, the owner's assistant) at the owner's direction. They were merged in PR #10 (merge commit `0972802`) and are public on main; the owner reviews and directs them. They were drafted from the official World ID pages cited in the spec; no prompts or session transcripts are published.
- The spec-hardening documentation changes (branch `docs-spec-hardening`: spec 01, 03 and 04 edits, the `EVIDENCE.md` expansion, the README and `AI_USAGE.md` wording fixes, and the new `docs/hackathon/95-submission-checklist.md` and `docs/hackathon/96-ai-artifact-plan.md`) were drafted with AI assistance (Grok Bot, the owner's assistant) at the owner's direction, as a draft pull request for the owner's review. Until the owner has reviewed and merged it, nothing here records that review. No prompts or session transcripts of that work are published; the [AI artifact plan](docs/hackathon/96-ai-artifact-plan.md) says how event prompts will be handled.
- PR #5 itself (this documentation change) was drafted with the help of an AI assistant, Grok Bot, the owner's assistant, and is reviewed by the owner before merge.

## Usage by area

| Area | AI tools used | AI contribution | Human contribution and review | Supporting artifacts |
|---|---|---|---|---|
| Architecture/specifications | Cursor, Claude Code, Grok Bot (across the project; not mapped per area); Kimi (review and documentation drafting/revision only) | AI assisted drafting, including the pre-kickoff specs in `docs/hackathon/` (PR #3, PR #4) | The owner designed the architecture and decided; the owner reviewed and edited the pre-kickoff specs (PR #3, PR #4) | `docs/hackathon/01-x402-arc-gateway.md`, `docs/hackathon/02-world-id-gating.md`, `docs/hackathon/03-ens-integration.md`, and, from the Agent Pass documentation PR, `docs/hackathon/04-agent-pass.md` and `docs/hackathon/90-stretch-options.md`. Git commit messages. Sanitized specs: see Published AI artifacts below. Prompts: none published yet. |
| Backend/indexer code | Cursor, Claude Code, Grok Bot (across the project; not mapped per area) | AI assisted backend and indexer code. Public Git history records commits and file changes. Private Cursor and Claude Code session history provides additional provenance where available. Together they do not constitute a complete per-file AI/authorship map, and exact per-file AI proportions were not tracked. | The owner designed the architecture, made operational decisions and operated the stack, reviewed AI-written code, ran tests and verified results, and wrote some code by hand | Git commit messages. Private Cursor and Claude Code session history (not published). Sanitized artifacts: None published yet; sanitized artifacts may be added after review for secrets. |
| Documentation | Cursor, Claude Code, Grok Bot (across the project; not mapped per area); Kimi (review and documentation drafting/revision only) | AI assisted documentation, including PR #3, PR #4 and PR #5 | The owner reviewed AI-written documentation and edited the PR #3 and PR #4 documentation. The teammate owns pricing, documentation and positioning on the business side | Git commit messages. Planning documents: `DEMO_PLAN.md`, `EVIDENCE.md`, see Published AI artifacts below. Prompts: none published yet. |
| Tests and CI | Cursor, Claude Code, Grok Bot (across the project; not mapped per area) | AI assisted tests and CI. CI currently only builds and vets the Go services; it does not run tests | The owner ran tests and verified results | Git commit messages. Evidence register and test-plan template (not results): `EVIDENCE.md`, see Published AI artifacts below. Prompts: none published yet. |
| UI/media/assets | Origin not confirmed for the existing assets; the project-wide tool list above is not mapped to them | Origin and AI involvement not confirmed for the existing assets; git history does not record it | The teammate owns the demo and submission video. Review of the existing UI and media assets: not confirmed. Git records `ozzyshaik` as author and committer; this does not establish who designed the assets or whether AI assisted them (see the inventory below) | UI/media asset inventory below |

## Published AI artifacts

These are specification, planning and test-plan documents that were already public in this repository. This file states above that the pre-kickoff specifications and documentation were drafted with AI assistance and reviewed by the owner, and listing them here adds no new content. Each was scanned on 2026-10-27 for credentials and keys, connection strings, private hostnames and IP addresses, filesystem paths and sensitive server configuration, and none was found.

The Agent Pass specification and the stretch-options document are new in the pre-kickoff Agent Pass documentation PR and were merged in PR #10 (merge commit `0972802`), so they are public on main. They were scanned on 2026-10-27, before being listed, with pattern searches for private filesystem paths, IPv4 and IPv6 addresses, email addresses, token, key and secret patterns (JWT-like strings, private key headers, long hexadecimal and base64-like strings, `0x` addresses, `secret`, `token`, `password` and `api key` assignments), hostnames and URLs, and infrastructure terms (reverse proxy, service manager, port and connection-string patterns). The lines added to the README, CONTINUITY, EVIDENCE, DEMO_PLAN and specs 02 and 03 in the same PR, and the revised text of the Agent Pass specification, were scanned with the same patterns again after the PR's revision commits (README IDKit retargeting, the CONTINUITY scope and prize-table edits, and the Agent Pass specification corrections), together with this file's own added lines. Result: nothing found. The re-run after the revision commits matched only prose, namely the heading words "ID token validation:" (the `token` pattern) and this paragraph's own description of the scan patterns; neither is a credential, path, host or address. The only URLs and hostname present are public official pages (`auth.world.org`, `ethglobal.com`, and the World ID `acr` class identifiers under `world.org`), cited as sources.

| Artifact | Kind | Status |
|---|---|---|
| `docs/hackathon/01-x402-arc-gateway.md` | specification (planned, not implemented) | public in the repository, scan passed |
| `docs/hackathon/02-world-id-gating.md` | specification (planned, not implemented) | public in the repository, scan passed |
| `docs/hackathon/03-ens-integration.md` | specification (planned, not implemented) | public in the repository, scan passed |
| `docs/hackathon/04-agent-pass.md` | specification (planned, not implemented) | added in the Agent Pass documentation PR; merged (`0972802`) and public on main; scan passed |
| `docs/hackathon/90-stretch-options.md` | uncommitted stretch options (planned, not implemented, not committed) | added in the Agent Pass documentation PR; merged (`0972802`) and public on main; scan passed |
| `EVIDENCE.md` | evidence register and test-plan template (nothing run yet) | public in the repository, scan passed |
| `DEMO_PLAN.md` | demo plan (not an existing demo) | public in the repository, scan passed |
| `docs/hackathon/95-submission-checklist.md` | submission checklist (all boxes unchecked; nothing submitted) | added in the spec-hardening pull request, not on main until that PR is merged; scan result below |
| `docs/hackathon/96-ai-artifact-plan.md` | plan for preserving and publishing event prompts and planning artifacts (plan only; nothing published) | added in the spec-hardening pull request, not on main until that PR is merged; scan result below |

The spec-hardening pull request (draft, documentation only) also edits `docs/hackathon/01-x402-arc-gateway.md`, `03-ens-integration.md`, `04-agent-pass.md`, `EVIDENCE.md` and `README.md`. Their rows above describe the versions on main until it is merged. The lines it adds to all of these files and to this file were scanned on 2026-10-27 with the same pattern families as above (private filesystem paths, IPv4 and IPv6 addresses, email addresses, JWT-like strings, private key headers, long hexadecimal and base64-like strings, `0x` addresses, `secret`, `token`, `password` and `api key` assignments, hostnames and URLs, and infrastructure terms). Result: no private path, email address, token, key or private hostname was found. The matches were: `127.0.0.1` (a loopback literal quoted from the code in the README known limits); `0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045`, a widely published public Ethereum address used as a test input in the fingerprint vectors, together with SHA-256 hashes of synthetic request strings; port numbers (`:8081`, `:8090`, `:8091`) that are already listed in the public README; and public URLs on `developers.circle.com`, `eips.ethereum.org` and `ethglobal.com`. A re-run over this file's own added lines matched only this paragraph's description of the patterns and the address and hash named above. This is a pattern scan plus a read of the matches, not a guarantee.

Prompts: none published yet. No prompt or session transcript has been added; private Cursor and Claude Code session history is not published. Sanitized prompts may be added after review for secrets. The plan for doing that is in [docs/hackathon/96-ai-artifact-plan.md](docs/hackathon/96-ai-artifact-plan.md); it publishes nothing by itself.

Artifacts generated during the event (after the tag `ethglobal-mumbai-2026-start` exists) can be appended here during the hackathon, with their own scan.

## UI/media asset inventory

Scope: the UI, diagrams, images and other media that exist in the repository at the time of writing (pre-start). Facts in this section come from the repository tree and its Git history (author, committer, date, commit message), not from owner confirmation. Git history does not say whether AI tools were involved in any of these assets, and it does not say who designed them. Where origin cannot be confirmed from Git, this file says "origin not confirmed" and does not guess.

| Path | Type | Size | Git evidence | Origin and AI/human involvement |
|---|---|---|---|---|
| `web/index.html` | Static HTML/CSS/JS page, "Contract CFG Visualizer"; served at `/` by `cmd/api`; loads d3 and d3-graphviz from public CDNs | 5,420 bytes (107 lines) | Added in `241af1f` "Initial public release" (tag `baseline-pre-ethglobal-mumbai`), author and committer `ozzyshaik`, 2026-10-24 15:57 IST. No later commit changes it | Origin not confirmed |
| `sgip/main.py`, embedded `DASHBOARD_HTML` (lines 650-1365) | HTML/CSS/JS dashboard served by `sgip` at `/` and `/embed`; loads Cytoscape.js and dagre from public CDNs | about 20.9 KB inside a 46,157-byte Python file | Added in `241af1f`, same author, committer and date. No later commit changes the file | Origin not confirmed |
| `cmd/rpc-gateway/main.go`, embedded `swaggerHTML` (lines 331-363) | HTML page loading Swagger UI from a public CDN; served at `/docs` | about 1 KB inside a 57,110-byte Go file | Added in `241af1f`, same author, committer and date. No later commit changes the file | Origin not confirmed |
| `README.md`, Mermaid architecture flowchart | Diagram as Mermaid text rendered by GitHub; no image file | 773 bytes of text inside README.md | Added in `241af1f`, same author, committer and date | Origin not confirmed |
| `docs/hackathon/01-x402-arc-gateway.md`, Mermaid state diagram | Diagram as Mermaid text rendered by GitHub; no image file | 562 bytes of text inside the spec | Added in `e21e072` "docs: apply owner review: Arc fulfillment state machine, World ID recovery and revocation, scope wording", author and committer `ozzyshaik`, 2026-10-26 10:27 IST. The commit message records an owner review of the spec; it does not say who drew the diagram or whether AI was used | Origin not confirmed. The spec it belongs to is covered by the AI-assistance statement above for PR #3 and PR #4 |

What the inventory could not confirm:

- Who designed or wrote each UI page and diagram, and whether any AI tool was involved. The commits are the public snapshot commit and a documentation commit; the private development history before 2026-10-24 is not public, so it cannot be used as evidence here.
- Whether the UI pages were reviewed by a human, beyond the general statement above that the owner reviewed AI-written code.

What does not exist: no raster or vector image files (png, jpg, gif, svg, webp, ico), icons, fonts, audio, video, slide decks or design files are in the repository tree or in any commit reachable from any ref (checked 2026-10-27). The demo and submission video, any Arc Studio frontend and any architecture diagram prepared for the event do not exist in the repository yet.

Assets generated during the event (after the tag `ethglobal-mumbai-2026-start` exists) can be appended to this inventory during the hackathon, with their own origin and AI-involvement entries. They will not be backfilled into the rows above.

## Baseline versus event assistance

Baseline assistance (this file, as written before the event) and event assistance will be recorded separately once the event
begins. Event entries will be added after the tag `ethglobal-mumbai-2026-start` exists and will not be backfilled into the
baseline rows above.
