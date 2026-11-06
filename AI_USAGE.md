# AI usage

Status: baseline and separately identified event-period documentation, based on facts confirmed by the owner. Nothing in this file is guessed. No percentages are
claimed. Per-file / per-part AI attribution below is limited to the event-period `x402-seller/` package at draft PR #23 head `f79de2f`; elsewhere, no complete per-file map is claimed.

This file covers both baseline assistance (everything up to the annotated tag `baseline-pre-ethglobal-mumbai` and the pre-kickoff
changes merged before the event, as listed in [CONTINUITY.md](CONTINUITY.md)) and separately identified event-period assistance.
The annotated start tag
`ethglobal-mumbai-2026-start` now exists (tag object `7993c080fa47d3f15e584795fcb4bd59c0250012`, peeled commit
`cb39dbb3d2dbf217949ef279c7e526c543c0b7ea`); it must never be moved or backdated. Baseline assistance and event assistance
are separated: under Corrected Variant B, event-period entries begin with the administrative/documentation merge of PR #21
and continue for commits after the start tag. See [CONTINUITY.md](CONTINUITY.md) for the baseline/event split.

## Confirmed facts

- AI tools used across the project: Cursor, Claude Code, Grok Bot and Kimi. This file does not map each tool to an area.
- Kimi's role: Kimi assisted repository review, continuity and specification review, documentation drafting/revision, and product-website drafting. Recommendations and wording from these reviews were incorporated into the pre-kickoff documentation. Kimi proposes repository changes through pull requests and has not operated the infrastructure.
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
- The spec-hardening documentation changes (branch `docs-spec-hardening`, pull request #11: edits to specs 01, 03 and 04, the `EVIDENCE.md` expansion, `DEMO_PLAN.md`, the README and `AI_USAGE.md` wording fixes, and the new `docs/hackathon/95-submission-checklist.md` and `docs/hackathon/96-ai-artifact-plan.md`) were drafted with AI assistance (Grok Bot, the owner's assistant) at the owner's direction, including a second revision that replaced the verify-then-fulfill-then-batch payment design with a settle-before-serve design. PR #11 was merged normally at `c3d58a5`; this records the public merge, not an additional claim about review. It was documentation only, and nothing its feature specifications describe has thereby been implemented or run. No prompts or session transcripts of that work are published; the [AI artifact plan](docs/hackathon/96-ai-artifact-plan.md) says how event prompts will be handled.
- The investigation/evidence documentation change (the new `docs/hackathon/05-investigation-evidence.md`, revisions to `README.md`, `DEMO_PLAN.md`, `EVIDENCE.md` and `docs/hackathon/95-submission-checklist.md`, and the post-merge status corrections in this file) was drafted with AI assistance (Kimi) after review of public ETHGlobal ceremony, showcase, prize and repository material and of this repository. This paragraph records drafting only, not owner review or merge. The change is documentation/specification only: it implements no fixture, verification script, feature or UI, creates no tag, changes no live system and adds no evidence result. No prompt or session transcript is published by it.
- The product-led README revision in PR #13 was drafted with AI assistance (Kimi) and merged normally at `30b9caacee9916a103401255e844c1e78bf1b157`. This records drafting and the public merge, not an additional claim about human review. Only `README.md` changed; no infrastructure was accessed or modified.
- The `website/` product site, its Node content tests and documentation, and the accompanying website/Continuity/AI-provenance edits were drafted with AI assistance (Kimi) at the owner's direction. The pull request contains the public source and diff; this paragraph claims no human review, approval or merge. The website is pre-kickoff product/presentation work, not the Arc Studio event frontend or a partner integration. Its records are synthetic and its roadmap capabilities remain planned. No prompt or session transcript is published by this change.
- PR #5 was an earlier documentation change drafted with AI assistance (Grok Bot, the owner's assistant). This file does not record a separate review of it beyond the statements above.
- The existing-core corrections in PR #18 (API-key sync, `cmd/api` `/ws` fan-out, address normalization on `/v1/balance` and the `cmd/api` contract routes, a bounded ExEx DB-receipt wait, and `go test` in CI) and PR #19 (`cmd/api` `/ws` Gatekeeper failing closed with throttled Valkey client retries; `rpc-gateway` API-key lookups with `FINAL`, a 10 s key cache and 503 for failed lookups), each with its tests and documentation, and the CI-only change in PR #20 (installing `redis-server` in CI and running the Go tests verbosely), were drafted with AI assistance (Claude Code) at the owner's direction. PR #18 was merged normally at `39e6c86cf5e617b29b7e9eeab6a833c1564c21d9`, PR #19 at `90d11089f2b145e251db48edda6173a5a2f3f800` and PR #20 at `56ad09ad82640cef65a7909f4177548cd6a574dd`. This records drafting and the public merges, not an additional claim about human review, runtime verification or test results; each pull request contains its own diff and validation report. No prompt or session transcript is published by this change.
- PR #21 (documentation and website wording: Continuity ledger updates, AI_USAGE/OpenAPI wording, website limits text, evidence IDK-07 correction after PR #19) was drafted with AI assistance (Claude Code) at the owner's direction. Its seven branch commits (`c65f36f` through `e4b6346`) were authored before the official hacking start (2026-11-05 21:00:00 IST) and remain pre-existing work under Corrected Variant B. Kimi (an AI agent) reviewed the public diff and checks. The owner authorized the merge; GitHub executed the merge from the owner's account at `cb39dbb3d2dbf217949ef279c7e526c543c0b7ea` (2026-11-05 21:17 IST). This does **not** claim that the owner personally reviewed the code. Kimi's runtime/deployment verification was limited to the public static site's HTTP response and the deployed JavaScript wording. No production API, live Valkey/ClickHouse, ExEx end-to-end runtime, or public-versus-production runtime-equivalence verification is claimed for PR #21. Under Corrected Variant B, only the merge commit is event-period administrative/documentation work; no part of PR #21 is claimed as an event-built feature. No prompt or session transcript is published by this change.
- The annotated start tag `ethglobal-mumbai-2026-start` (tag object `7993c080fa47d3f15e584795fcb4bd59c0250012`, peeled commit `cb39dbb3d2dbf217949ef279c7e526c543c0b7ea`) was created by Claude (the tagger) and pushed by this assistant (Grok Bot) on the owner's explicit authorization. The tag must never be moved or backdated.

## Usage by area

| Area | AI tools used | AI contribution | Human contribution and review | Supporting artifacts |
|---|---|---|---|---|
| Architecture/specifications | Cursor, Claude Code, Grok Bot (across the project; not mapped per area); Kimi (review and documentation drafting/revision only) | AI assisted drafting, including the pre-kickoff specs in `docs/hackathon/` (PR #3, PR #4) | The owner designed the architecture and decided; the owner reviewed and edited the pre-kickoff specs (PR #3, PR #4) | `docs/hackathon/01-x402-arc-gateway.md`, `docs/hackathon/02-world-id-gating.md`, `docs/hackathon/03-ens-integration.md`, and, from the Agent Pass documentation PR, `docs/hackathon/04-agent-pass.md` and `docs/hackathon/90-stretch-options.md`. Git commit messages. Sanitized specs: see Published AI artifacts below. Prompts: none published yet. |
| Backend/indexer code | Cursor, Claude Code, Grok Bot (across the project; not mapped per area) | AI assisted backend and indexer code. Public Git history records commits and file changes. Private Cursor and Claude Code session history provides additional provenance where available. Together they do not constitute a complete per-file AI/authorship map, and exact per-file AI proportions were not tracked. | The owner designed the architecture, made operational decisions and operated the stack, reviewed AI-written code, ran tests and verified results, and wrote some code by hand | Git commit messages. Private Cursor and Claude Code session history (not published). Sanitized artifacts: None published yet; sanitized artifacts may be added after review for secrets. |
| Documentation | Cursor, Claude Code, Grok Bot (across the project; not mapped per area); Kimi (review and documentation drafting/revision only) | AI assisted documentation, including PR #3, PR #4 and PR #5 | The owner reviewed AI-written documentation and edited the PR #3 and PR #4 documentation. The teammate owns pricing, documentation and positioning on the business side | Git commit messages. Planning documents: `DEMO_PLAN.md`, `EVIDENCE.md`, see Published AI artifacts below. Prompts: none published yet. |
| Tests and CI | Cursor, Claude Code, Grok Bot (across the project; not mapped per area) | AI assisted tests and CI. CI builds, vets and tests the Go services (`go test ./cmd/...`, added in PR #18; since PR #20 the CI workflow installs `redis-server` and runs `go test -v -count=1 ./cmd/...`, so the log lists every test) and runs the website content checks and production build. Go tests that need a Valkey-compatible server start a local `redis-server` and are skipped where it is not installed; in CI they run, because the workflow installs it. They test compatibility with `redis-server`, not a Valkey deployment. Website checks were also run locally | The owner ran tests and verified results | Git commit messages. Evidence register and test-plan template (not results): `EVIDENCE.md`, see Published AI artifacts below. Prompts: none published yet. |
| UI/media/assets | Origin not confirmed for the existing assets; the project-wide tool list above is not mapped to them | Origin and AI involvement not confirmed for the existing assets; git history does not record it | The teammate owns the demo and submission video. Review of the existing UI and media assets: not confirmed. Git records `ozzyshaik` as author and committer; this does not establish who designed the assets or whether AI assisted them (see the inventory below) | UI/media asset inventory below |
| Product website (`website/`) | Kimi | Kimi drafted the frontend, synthetic records, procedural visual source, pitch content, Node content tests and website documentation at the owner's direction | Owner direction is recorded; no human review, approval or merge is claimed by this entry | `website/`, its pull-request diff, `npm run check` and `npm run build` outputs; no prompts or session transcripts published |

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
| `docs/hackathon/95-submission-checklist.md` | submission checklist (all boxes unchecked; nothing submitted) | added in PR #11, merged normally (`c3d58a5`) and public on main; scan result below |
| `docs/hackathon/96-ai-artifact-plan.md` | plan for preserving and publishing event prompts and planning artifacts (plan only; nothing published) | added in PR #11, merged normally (`c3d58a5`) and public on main; scan result below |
| `docs/hackathon/05-investigation-evidence.md` | supporting investigation/reproducibility specification (planned, not implemented) | added by the investigation/evidence documentation change; scan result below |

The spec-hardening pull request (#11, documentation only, merged normally at `c3d58a5`) also edited `docs/hackathon/01-x402-arc-gateway.md`, `03-ens-integration.md`, `04-agent-pass.md`, `EVIDENCE.md`, `DEMO_PLAN.md` and `README.md`. Before merging, every line the pull request added relative to its base (`0972802`), in all of these files and in this file, was scanned on 2026-10-27 after its second revision, and the full set of added lines was scanned again on 2026-10-28 after the correction pass described below, with pattern searches for private filesystem paths, IPv4 and IPv6 addresses, email addresses, JWT-like strings, private key headers and common token prefixes, long hexadecimal and base64-like strings, `0x` addresses, `secret`, `token`, `password` and `api key` assignments, URLs and hostnames, and infrastructure terms (reverse proxy, service manager, ports, connection strings). Result: no private path, email address, token, key, connection string or private hostname was found. The matches were: `127.0.0.1` (a loopback literal quoted from the code in the README known limits); `0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045`, a widely published public Ethereum address used as a test input in the fingerprint vectors, and SHA-256 hashes of synthetic request strings (the long-hexadecimal matches, together with the path segments of the public Circle documentation URLs, which matched the base64-like pattern); port numbers (`:8081`, `:8090`, `:8091`) already listed in the public README; public URLs on `developers.circle.com`, `eips.ethereum.org` and `ethglobal.com`; and the words `secret`, `token`, `password` and `api key` in prose (requirements such as "no secret reaches logs" and redaction instructions), none of them an assignment or a value. This is a pattern scan plus a read of the matches, not a guarantee, and the scan was run by the same assistant that drafted the changes.

**Correction pass on draft #11 (2026-10-28, documentation only).** After review of head `d35d8e8`, one narrow correction pass was made on the same branch with normal incremental commits, drafted with AI assistance (Grok Bot) and to be reviewed by the owner. It changes documentation only: no code, no tag, no live system, and no evidence result was added; every `EVIDENCE.md` row is still `PLANNED` or `NOT_YET_RUN` with its Commit, Actual and Proof cells empty, and nothing in it has been run. The corrections: (1) the automatic `void` of an uncertain submission was removed, so `void` is reachable only from `submission_pending` with an attempt counter of zero, an empty transfer lookup is no longer treated as proof of non-acceptance, an attempted uncertain row is kept and flagged for an operator, a later matching transfer is adopted as accepted, and a new planned test (ARC-23, X-14) covers that case; (2) the ARC-04 retry procedure now starts after fulfillment; (3) ARC-06 passes only on `batched` or `confirmed` observed through transfer reconciliation, and `received` is not batching evidence; (4) the Go API wording is scoped to the ENS preflight and specs 02 and 04 instead of calling it unchanged; (5) the open-source wording in the README and the checklist is replaced with the statement quoted in those files; (6) the Agent Pass operation is documented as storing four payment terms, not the complete x402 requirements object. The same pattern scan was re-run over all lines added relative to main (`0972802`) in every changed file, including this file, on the final text of this pass. It found the same categories as the earlier scan and no new one: no private path, email address, token, key, connection string or private hostname; the matches were the loopback literal `127.0.0.1`, the published public test address and the SHA-256 hashes of synthetic strings, the port numbers already listed in the public README, public URLs on `developers.circle.com`, `eips.ethereum.org` and `ethglobal.com`, and the words `secret`, `token`, `password` and `api key` in prose. It is a pattern scan plus a read of the matches, not a guarantee, and it was run by the same assistant that drafted the changes.

**Investigation/evidence documentation scan (2026-10-28).** Every added line in the investigation/evidence documentation change, including the new specification and this file, was scanned for private filesystem paths, IPv4 addresses, private-key headers, JWT-like strings, common token prefixes, long hexadecimal strings, secret/token/password/API-key assignments, URLs and hostnames. No private path, credential, connection string, private hostname or new live infrastructure identifier was found. The only IP match was the loopback literal `127.0.0.1`, and the only long hexadecimal match was the widely published public Ethereum test address `0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045`; both were carried through replacement of the pre-existing scan paragraph above, not introduced as deployment data. This was a pattern scan plus a read of the matches by the same assistant that drafted the change, not a guarantee. `EVIDENCE.md` still has 77 rows (41 `PLANNED`, 36 `NOT_YET_RUN`), no duplicate IDs, and empty Commit, Actual and Proof cells; the four new investigation rows are plans, not results.

Prompts: none published yet. No prompt or session transcript has been added; private Cursor and Claude Code session history is not published. Sanitized prompts may be added after review for secrets. The plan for doing that is in [docs/hackathon/96-ai-artifact-plan.md](docs/hackathon/96-ai-artifact-plan.md); it publishes nothing by itself.

Artifacts generated during the event (after the tag `ethglobal-mumbai-2026-start` exists) can be appended here during the hackathon, with their own scan. Under the Continuity rule ("All new parts of extending an existing project must remain open source.", [ethglobal.com/rules](https://ethglobal.com/rules)), event-built artifacts that are part of the project stay in this public repository; credentials, private data and sensitive live-infrastructure details are redacted after review, and each removal is marked. No submitted feature or required implementation is hidden or misrepresented.

## UI/media asset inventory

Scope: the UI, diagrams, images and other media that exist in the repository at the time of writing (pre-start). Facts in this section come from the repository tree and its Git history (author, committer, date, commit message), not from owner confirmation. Git history does not say whether AI tools were involved in any of these assets, and it does not say who designed them. Where origin cannot be confirmed from Git, this file says "origin not confirmed" and does not guess.

| Path | Type | Size | Git evidence | Origin and AI/human involvement |
|---|---|---|---|---|
| `web/index.html` | Static HTML/CSS/JS page, "Contract CFG Visualizer"; served at `/` by `cmd/api`; loads d3 and d3-graphviz from public CDNs | 5,420 bytes (107 lines) | Added in `241af1f` "Initial public release" (tag `baseline-pre-ethglobal-mumbai`), author and committer `ozzyshaik`, 2026-10-24 15:57 IST. No later commit changes it | Origin not confirmed |
| `sgip/main.py`, embedded `DASHBOARD_HTML` (lines 650-1365) | HTML/CSS/JS dashboard served by `sgip` at `/` and `/embed`; loads Cytoscape.js and dagre from public CDNs | about 20.9 KB inside a 46,157-byte Python file | Added in `241af1f`, same author, committer and date. No later commit changes the file | Origin not confirmed |
| `cmd/rpc-gateway/main.go`, embedded `swaggerHTML` (lines 331-363) | HTML page loading Swagger UI from a public CDN; served at `/docs` | about 1 KB inside a 57,110-byte Go file | Added in `241af1f`, same author, committer and date | Origin not confirmed |
| `README.md`, Mermaid architecture flowchart | Diagram as Mermaid text rendered by GitHub; no image file | 773 bytes of text inside README.md | Added in `241af1f`, same author, committer and date | Origin not confirmed |
| `docs/hackathon/01-x402-arc-gateway.md`, Mermaid state diagram | Diagram as Mermaid text rendered by GitHub; no image file | 562 bytes of text inside the spec | Added in `e21e072` "docs: apply owner review: Arc fulfillment state machine, World ID recovery and revocation, scope wording", author and committer `ozzyshaik`, 2026-10-26 10:27 IST. The commit message records an owner review of the spec; it does not say who drew the diagram or whether AI was used | Origin not confirmed. The spec it belongs to is covered by the AI-assistance statement above for PR #3 and PR #4 |

What the inventory could not confirm:

- Who designed or wrote each UI page and diagram, and whether any AI tool was involved. The commits are the public snapshot commit and a documentation commit; the private development history before 2026-10-24 is not public, so it cannot be used as evidence here.
- Whether the UI pages were reviewed by a human, beyond the general statement above that the owner reviewed AI-written code.

Historical inventory as of 2026-10-27, before the product website was added: no raster or vector image files (png, jpg, gif, svg, webp, ico), icons, fonts, audio, video, slide decks or design files were in the repository tree or in any commit reachable from any ref checked on that date. At that date, the demo and submission video, any Arc Studio frontend and any architecture diagram prepared for the event did not exist in the repository.

Product website addition: `website/` adds HTML, CSS and JavaScript source, locally built Fontsource font assets, Lucide icons rendered as inline SVG, procedural Three.js models, code-generated diagrams, a nine-slide browser pitch and Node content tests. Kimi drafted these additions at the owner's direction; no human review or merge is claimed by this entry. The website does not add a raster/video asset, a recorded demo, an Arc Studio frontend or a partner-integration result. The historical statement above is not being backdated or applied to the new tree.

Assets generated during the event (after the tag `ethglobal-mumbai-2026-start` exists) can be appended to this inventory during the hackathon, with their own origin and AI-involvement entries. They will not be backfilled into the rows above.

## Baseline versus event assistance

Baseline assistance remains the rows above for work before the official hacking start. Event assistance is recorded separately
after the tag `ethglobal-mumbai-2026-start` exists and is not backfilled into the baseline area rows. Under Corrected Variant B,
the first event-period AI-provenance note is the PR #21 merge authorization and limited Kimi static-site check above; further
event entries are appended below as event work lands (including the `x402-seller/` table in this file).

## Event assistance: experimental x402-seller checkpoint (`f79de2f`)

Scope: draft PR [#23](https://github.com/ozzyshaik-org/world-supersonic-public/pull/23) head `f79de2f36b8a07371b072398ea1e2657775c3a2f` on branch `feat/arc-x402-seller` (unmerged). Event-built after `ethglobal-mumbai-2026-start`. This is an **experimental seller-engine checkpoint** only.

### Provenance method (what was checked; nothing guessed)

- `git ls-tree -r --name-only f79de2f -- x402-seller/` lists the files in the table below.
- `git log` authorship for every commit that introduced or changed those files (`5dee2bf`…`f79de2f`) is `ozzyshaik <ozzyshaik@users.noreply.github.com>` only.
- Commit messages describe the change (scaffold, engine, FAKE Gateway tests, x402 v2 validation, lease fencing, race CAS fix, CI job). They do **not** name Claude, Cursor, Kimi, Copilot or another tool as the producer of these files.
- Session context for this package: **Grok Bot executor sessions** wrote the code, tests and package docs under owner direction. No other AI tool is recorded in the commit messages for this package; this file does not invent one.

### Per-file / per-part AI attribution (`x402-seller/` at `f79de2f`)

| Path | AI tool that produced this part | Evidence noted |
|---|---|---|
| `x402-seller/.env.example` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/.gitignore` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/README.md` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/docs/CIRCLE_RECHECK.md` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/package-lock.json` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/package.json` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/authKey.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/chainConfig.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/fingerprint.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/gateway/circleAdapter.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/gateway/client.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/gateway/fakeGateway.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/index.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/ledger/memoryLedger.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/paymentValidation.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/priceTable.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/seller.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/transferMatch.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/types.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/upstream/client.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/src/upstream/fakeUpstream.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/test/authKey.test.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/test/circleImport.test.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/test/fingerprint.test.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/test/helpers.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/test/leaseRaceBudget.test.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/test/paymentStructure.test.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/test/regressionMatrix.test.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/test/stateMachine.test.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/tsconfig.json` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `x402-seller/vitest.config.ts` | Grok Bot executor sessions (code/tests/docs in this package) | git author `ozzyshaik`; commit messages name the change, not a second AI tool |
| `.github/workflows/ci.yml` (`x402-seller` job added on PR #23) | Grok Bot executor sessions | Same authorship trail; job runs `npm ci` / `typecheck` / `build` / `test` in `x402-seller/` |

### Owner contributions confirmed so far

Recorded only from the owner's own review / authorization messages (not invented coding credit):

- Specified requirements and review corrections for the seller engine, including the **10 PR #23 corrections** on 6 Nov 2026, the evidence-led correction pass, the **race-fix acceptance boundary** (ledger-level pre-fix proof only; no claim of a pre-fix full engine-level 2xx rejection; no live Arc/Circle verification), and the **frozen scope** to the paid `/v1/mev/analytics` slice.
- Independently verified `f79de2f` at local/unit level on 6 Nov 2026 Asia/Calcutta: typecheck and build passed; **116/116** tests passed; all five race tests passed; the new ledger regression fails on `dc761fd` with the expected `toBeNull` assertion; CI run `42180463805` reports success.

**Further owner contributions (hand-written code, line-level edits, or additional review credits) are pending owner confirmation and are not claimed here.**

### Event assistance: local HTTP adapter (`b03d41e`, draft PR #25)

Scope: draft PR [#25](https://github.com/ozzyshaik-org/world-supersonic-public/pull/25) head `b03d41ee7831dedc356590fd9170e5fec82f7afd` on `feat/x402-http-adapter` (base `feat/arc-x402-seller`, unmerged). Labels: `local-http-integration` + `unit-fake` Gateway — **not** live Arc evidence.

Provenance method: `git diff --name-status f79de2f..b03d41e -- x402-seller/` for new/changed paths; `git log` authorship for `6dc242c`…`b03d41e` is `ozzyshaik <ozzyshaik@users.noreply.github.com>` only; commit messages do not name Claude, Cursor, Kimi, Copilot or another tool. Session context: **Grok Bot executor sessions** wrote these parts under owner direction.

| Path | AI tool that produced this part | Evidence noted |
|---|---|---|
| `x402-seller/docs/CHECKPOINT.md` | Grok Bot executor sessions | new in `6dc242c` |
| `x402-seller/docs/CIRCLE_RECHECK.md` (edits) | Grok Bot executor sessions | modified on PR #25 |
| `x402-seller/README.md` (edits) | Grok Bot executor sessions | modified on PR #25 |
| `x402-seller/.env.example` (edits) | Grok Bot executor sessions | modified on PR #25 |
| `x402-seller/package.json` / `package-lock.json` | Grok Bot executor sessions | modified on PR #25 (incl. fast-check) |
| `x402-seller/src/index.ts` (exports) | Grok Bot executor sessions | modified on PR #25 |
| `x402-seller/src/http/config.ts` | Grok Bot executor sessions | new |
| `x402-seller/src/http/headers.ts` | Grok Bot executor sessions | new |
| `x402-seller/src/http/index.ts` | Grok Bot executor sessions | new |
| `x402-seller/src/http/server.ts` | Grok Bot executor sessions | new |
| `x402-seller/src/http/timeoutGateway.ts` | Grok Bot executor sessions | new |
| `x402-seller/src/http/upstreamHttp.ts` | Grok Bot executor sessions | new |
| `x402-seller/test/httpHeaders.test.ts` | Grok Bot executor sessions | new (6 unit-fake) |
| `x402-seller/test/httpPaymentHeader.property.test.ts` | Grok Bot executor sessions | new (3 unit-fake fast-check) |
| `x402-seller/test/httpAdapter.local-http-integration.test.ts` | Grok Bot executor sessions | new (12 local-http-integration + unit-fake Gateway) |

Owner contributions confirmed for this HTTP follow-up (requirements / acceptance only; not invented coding credit): authorized the thin HTTP adapter and local-http-integration test matrix in t253u; required unpaid 402, settle-before-upstream, 2xx, replay, Retry-After, redirect rejection, timeouts, bypass rejection, and FAKE-Gateway labelling. Further owner coding contributions remain pending owner confirmation.

### What this section does not claim

- No live Arc / Circle / Studio / durable-store / public-demo evidence. Local HTTP adapter evidence is `local-http-integration` + unit-fake Gateway only.
- No percentage of AI versus human authorship.
- No claim that "every rule" of the Arc specification is covered by an executable test unless that test exists and is recorded in [EVIDENCE.md](EVIDENCE.md).
