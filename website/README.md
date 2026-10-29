# Supersonic product website

Interactive product website and four-minute guided pitch for Supersonic: execution-level indexing, derived datasets, and investigation infrastructure for World Chain.

This is a **frontend-only product/presentation site**. It is not the planned Arc Studio event frontend, does not implement the hackathon integrations, and is not evidence of an Arc, World ID, Agent Pass, or ENS run. In the repository’s Continuity history, website commits made before `ethglobal-mumbai-2026-start` are pre-existing product/presentation work; the tag must not be created or moved by website work.

## Requirements

- Node.js 22.12 or newer
- npm
- A browser with JavaScript enabled; WebGL is optional because static fallbacks are provided

## Install and run

From this directory:

```sh
npm ci
npm run dev
```

Vite prints the local development address. No environment variables, credentials, chain endpoint, or production connection are required.

## Verify and build

```sh
npm run check    # Node content and response-contract tests
npm run build    # production output in dist/
npm run preview  # serve the production build locally
```

`npm test` is an alias for the same Node test suite used by `npm run check`. The build script is intentionally only `vite build`; validation remains a separate, visible step.

The site uses `base: './'` so the production bundle can be hosted at a subpath. No public deployment URL is asserted by this source tree.

## Product experience

- Procedural Three.js execution-field hero with a static poster fallback and reduced-motion handling.
- Four-chapter synthetic investigation story: **Detect → Trace → Verify → Respond**.
- Product comparison that positions Supersonic as an integrated, opinionated World Chain workflow without claiming that broader platforms cannot support equivalent workflows.
- Investigation workspace with finding filters, transaction selection, a six-step call graph, playback, scrubbing, and reset controls.
- Separate response views for a one-item `/v1/mev/analytics` array and SGIP `meta`. They are displayed together for explanation, not represented as a composite production endpoint. Copying the derived JSON is optional and works without a backend.
- Interactive architecture explorer, two orbitable procedural 3D models, and inspectable data-lineage/payment diagrams.
- Source-level proof register linking product claims to immutable public-code references.
- Responsive mobile compositions, keyboard-operated tabs, visible focus states, and reduced-motion support.

## Guided pitch mode

Open it with any **4-minute pitch** / **pitch mode** button, or deep-link to a slide:

```text
?pitch=1&slide=1
?pitch=1&slide=9
```

The deck contains nine slides with a suggested duration of exactly 240 seconds. Autoplay is optional; manual slide navigation stops it. Speaker notes include a source link for each slide.

| Control | Action |
|---|---|
| Right Arrow / Page Down | Next slide |
| Left Arrow / Page Up | Previous slide |
| Home / End | First / last slide |
| O | Toggle slide overview |
| N | Toggle speaker notes |
| F | Request or exit fullscreen when the browser permits it |
| Space | Play / pause autoplay when focus is not on a button or link |
| Escape | Close the active panel first; otherwise leave pitch mode |

The footer shows slide-specific and overall timing. The final slide can close the deck and move directly to the investigation workspace. Fullscreen availability depends on browser policy; refusal disables only that control.

## Evidence states

Every capability belongs to one of these states; the visual treatment never upgrades one state into another.

| State | Meaning |
|---|---|
| Public core | Source exists in the public Supersonic repository. This does not imply production parity or end-to-end runtime verification. |
| Dated measurement | Self-reported, read-only operator measurement with its date and limitations. Not a live counter or independent benchmark. |
| Synthetic preview | Deliberately illustrative identifiers and values. No chain query, payment, identity check, or settlement occurs. |
| Planned specification | Documented future work only. No implementation, partner run, or result is claimed. |
| Known boundary | A limitation that must remain visible beside the related claim. |

The website has no accounts, database, server-side persistence, or cross-device state. Pitch deep links are ordinary URL parameters; no user data is stored.

## Source-level reference map

References below use public commit `30b9caacee9916a103401255e844c1e78bf1b157` so the audited contract does not drift with a branch.

| Website claim or example | Public reference |
|---|---|
| Reth Execution Extension stream | [`exex/src/main.rs`](https://github.com/ozzyshaik-org/world-supersonic-public/blob/30b9caacee9916a103401255e844c1e78bf1b157/exex/src/main.rs) |
| MEV response field contract | [`cmd/rpc-gateway/main.go#L82-L90`](https://github.com/ozzyshaik-org/world-supersonic-public/blob/30b9caacee9916a103401255e844c1e78bf1b157/cmd/rpc-gateway/main.go#L82-L90) |
| `/v1/mev/analytics` handler and array response | [`cmd/rpc-gateway/main.go#L1049-L1139`](https://github.com/ozzyshaik-org/world-supersonic-public/blob/30b9caacee9916a103401255e844c1e78bf1b157/cmd/rpc-gateway/main.go#L1049-L1139) |
| SGIP `/api/tx/{tx_hash}` graph and metadata | [`sgip/main.py#L259-L351`](https://github.com/ozzyshaik-org/world-supersonic-public/blob/30b9caacee9916a103401255e844c1e78bf1b157/sgip/main.py#L259-L351) |
| Public ExEx verification boundary | [`exex/README.md#L7-L20`](https://github.com/ozzyshaik-org/world-supersonic-public/blob/30b9caacee9916a103401255e844c1e78bf1b157/exex/README.md#L7-L20) |
| Dated scale readings and known limits | [`README.md#what-is-live-and-what-is-not-known-limits`](https://github.com/ozzyshaik-org/world-supersonic-public/blob/30b9caacee9916a103401255e844c1e78bf1b157/README.md#what-is-live-and-what-is-not-known-limits) |
| Planned public fixture and one-command verification | [`docs/hackathon/05-investigation-evidence.md`](https://github.com/ozzyshaik-org/world-supersonic-public/blob/30b9caacee9916a103401255e844c1e78bf1b157/docs/hackathon/05-investigation-evidence.md) — specification only, not a run or result |

## Source organization

| Path | Responsibility |
|---|---|
| `index.html` | Semantic page, workspace, proof register, and pitch dialog structure |
| `src/investigation.js` | Shared synthetic records and field-shaped response helpers |
| `src/pitch-content.js` | Nine-slide narrative, timing, notes, and source references |
| `src/pitch.js` | Pitch navigation, panels, focus management, fullscreen, autoplay, and deep links |
| `src/story.js` | Four-chapter synthetic product story |
| `src/scene.js`, `src/showcase.js` | Procedural Three.js scenes and inspectors |
| `src/blueprints.js` | Data-lineage and planned-payment diagrams |
| `tests/content.test.js` | Narrative, identity, timing, response-field, and disclosure invariants |

Fonts are installed locally through Fontsource. Models and diagrams are generated from code; the site does not depend on external stock imagery, remote model assets, or a third-party runtime API.

Repository: <https://github.com/ozzyshaik-org/world-supersonic-public>
