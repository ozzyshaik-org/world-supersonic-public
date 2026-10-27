# Submission checklist

Status: template. Every box is unchecked: nothing below has been done, submitted, selected or verified. This checklist claims no eligibility, selection or prize. It is updated only when a step is actually completed, and it is read together with [EVIDENCE.md](../../EVIDENCE.md) and the [AI artifact plan](96-ai-artifact-plan.md).

The wording below was read from the official pages on 2026-10-27. Event rules can change, so each source is read again before submitting. Where a detail could not be verified from a page, the item says "to be rechecked".

## Official sources
| Short name | Page | Used for |
|---|---|---|
| Details | [ETHGlobal Mumbai: submission details](https://ethglobal.com/events/mumbai/info/details) | Deadline, partner prize selection, AI rules, version control, video guidelines, judging |
| General rules | [ETHGlobal rules](https://ethglobal.com/rules) | Pre-existing work, tracks, version control |
| Prizes | [ETHGlobal Mumbai prizes](https://ethglobal.com/events/mumbai/prizes) | The partner list, ENS, Arkiv and Ethereum Foundation wording |
| Arc | [Arc prizes](https://ethglobal.com/events/mumbai/prizes/arc) | Arc bounties and qualification requirements |
| World | [World prizes](https://ethglobal.com/events/mumbai/prizes/world) | World ID Agent Pass and IDKit prizes and requirements |

## Deadline and submission form
- [ ] Submit through the Hacker Dashboard before the deadline: Saturday, 7 November 2026, 09:00 IST. Details: "Late submissions won't be accepted."
- [ ] Fill in the form's essentials: project title, description and the link to the repository (Details).
- [ ] Choose the submission option (Details): Finalist and Partner Prizes, or Partner Prizes Only. A finalist presents for 7 minutes: 4 for the demo and 3 for Q&A with the judges.

## Track and pre-existing work
- [ ] Confirm which track the submission is entered in, in the Hacker Dashboard and, if needed, with the organizers. The Arc "Arc Studio Continuity Track: Bring Your Project to Arc" prize and the $2,500 "World ID Agent Pass" prize are each labelled on the prizes pages as only available to Continuity Track participants. Whether this project is entered in a Continuity track, and what that means for each prize, is to be rechecked. Nothing here claims it.
- [ ] General rules, as read: a Classic "From Scratch" project that used pre-existing project-specific work is ineligible for partner prizes and the finalist category; a Continuity submission must "clearly document what work existed before the hackathon" and include substantive new work done during the event, and "eligibility for specific partner prizes may vary". Read them again at the time and follow the track that applies.
- [ ] Disclose pre-existing work in writing, as the rules require ("disclose any pre-existing work in writing to the ETHGlobal team and include full details in your submission (repo history, video, and description)"). The method for telling the ETHGlobal team is to be rechecked.
- [ ] Keep commit history across the event. Details: "Submissions with large single commits or missing histories may be disqualified."
- [ ] Event diff: the repository shows what is new and what existed before, using the comparison commands in [CONTINUITY.md](../../CONTINUITY.md) once the start tag exists. This checklist does not create, move or change any tag.

## Partner prize selections
- [ ] Select partner prizes on the last step of the submission form. Details: "you can select up to 3 Partner Prizes to apply for", and the selection "is the only way for partners to assess your project".
- [ ] Count partners, not tracks. Details: "If a partner has multiple tracks, you can be eligible for all of them while only counting as 1 Partner Prize."
- [ ] Arc and World are two partners, so they use two of the three selections. Selecting several World tracks (for example Agent Pass and IDKit) still uses one.
- [ ] Use the third selection only for an integration that genuinely meets that partner's qualification requirements, from ENS, Arkiv or Ethereum Foundation. Otherwise leave it unused.
  - [ ] ENS: ordinary `.eth` resolution alone does not support a claim. Re-read the ENS prize requirements on the Prizes page ([spec 03](03-ens-integration.md) states the condition).
  - [ ] Arkiv and Ethereum Foundation: no integration is planned in this repository. Select one only if one is actually built and meets that partner's requirements on the Prizes page.
- [ ] For each selected partner prize, explain how the partner's tools were used or integrated, give feedback, and share comments (Details).
- [ ] After submitting, open the Hacker Dashboard and check that the selected prizes and the stated Arc bounty are what was intended. The dashboard screens were not viewed when this was written: to be rechecked.

## Arc (Circle) requirements
Arc page: "Every Arc prize requires using Arc Studio as the starting point. Please state which bounty you're submitting for in your submission."
- [ ] Explicitly select and state the Arc bounty being submitted for in the submission text. The Arc Studio Continuity bounty is stated only if the track and eligibility above are confirmed.
- [ ] Arc Studio project URL. [Spec 01](01-x402-arc-gateway.md) names `studio.arc.io` and the Arc prizes page links `studio.arc.com`: to be rechecked, and the URL in the submission is the real project URL.
- [ ] Arc Studio shown in the demo.
- [ ] A working frontend and a working backend, shown together. [Spec 01](01-x402-arc-gateway.md) counts the Studio-built web UI as the frontend and the seller service as the backend; the CLI utility does not count as the frontend.
- [ ] An architecture diagram (client, seller service, Circle Gateway, Go API, Arc).
- [ ] A video demonstration and presentation covering the project's core functions and its use of Arc Studio and Circle's developer tools, supported by documentation.
- [ ] A link to the GitHub repository.
- [ ] Integration feedback on Arc Studio, Circle Gateway and Arc Testnet.

## World requirements
- [ ] World ID Agent Pass ([spec 04](04-agent-pass.md)): the complete journey (identity request, user completion, validated result, protected action); a denied, expired, cancelled or otherwise unsuccessful path where the protected action does not occur; identity results validated in a secure backend; a short integration debrief.
- [ ] Best Use of IDKit ([spec 02](02-world-id-gating.md)): IDKit in a functioning app; a supported credential verified on the server; an explanation of the trust moment and why the credential is the minimum sufficient assurance; a successful verification and one meaningful alternative path; a short integration debrief.
- [ ] IDKit debrief, written from real experience after the integration is attempted: time to first success, friction encountered, missing capability or documentation, and the one improvement with the greatest impact.
- [ ] Agent Pass debrief, written the same way and kept separate from the IDKit debrief.
- [ ] Which World prizes are entered is decided against the Prizes page and the track confirmation above. The Agent Pass and IDKit evidence is kept separate ([EVIDENCE.md](../../EVIDENCE.md)).

## Code, demo and AI
- [ ] A live demo that works at the time of judging.
- [ ] Public code: a public repository with an open-source license, and the README sections the specs require.
- [ ] AI attribution: where and how AI tools were used, by part, file or asset. Details: "Clearly document in your submission where and how AI tools were used in the project." See [AI_USAGE.md](../../AI_USAGE.md).
- [ ] Meaningful human contribution is explicit and true. Details: "Submissions that rely entirely on AI without meaningful contributions from team members may not be eligible for partner prizes or finalist consideration."
- [ ] Spec-driven artifacts are in the repository, as in the [AI artifact plan](96-ai-artifact-plan.md). Details: "you must include all spec files, prompts, and planning artifacts in your submission repository."
- [ ] No secret, key, token or private data appears in the repository, the video, the screenshots or the written submission.

## Demo video
Details lists the video as optional ("While optional, we strongly encourage you to include a 2-4 minute demo video"), but the Arc requirements ask for a "video demonstration + presentation". This checklist therefore treats the video as required for the Arc submission.
- [ ] Length between 2 and 4 minutes. Details: videos under 2 or over 4 minutes "will be automatically rejected during upload".
- [ ] Exported at 720p or higher.
- [ ] Not sped up to fit the limit.
- [ ] The narrator talks. No music with on-screen text in place of talking.
- [ ] Not recorded on a mobile phone.
- [ ] No text-to-speech voice and no AI voiceover.
- [ ] Shows the project in action, with unnecessary waiting edited out; keeps the introduction short; at most four bullets per slide.
- [ ] Shows Arc Studio and the Studio project, the frontend and backend working together, and the World flows being submitted.
- [ ] Reviewed frame by frame for secrets and private data before upload.
- [ ] The video is featured on the ETHGlobal Showcase (Details), so it is public-ready.

## Before the final submission
- [ ] No required artifact is left pending at final completion. Every row in [EVIDENCE.md](../../EVIDENCE.md) for a feature that is claimed is either replaced by a real, recorded result or the claim is removed from the submission. `PLANNED` and `NOT_YET_RUN` do not remain on a claimed feature.
- [ ] The debrief templates in specs 02 and 04 are completed from real experience, or the matching prize is not claimed.
- [ ] The Arc Studio URL, demo video, architecture diagram, live demo URL, public repository and AI artifacts are all present and open without logging in.
- [ ] README, EVIDENCE.md, AI_USAGE.md and the written submission say the same thing, and none claims selection, eligibility or winnings that the organizers have not confirmed.
- [ ] Each "to be rechecked" item above has been checked against the official page, and the answer recorded here.
