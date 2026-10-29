import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { findings, derivedResponse, derivedResponsePayload, sgipMetadata } from '../src/investigation.js';
import { slides, PITCH_TOTAL_SECONDS } from '../src/pitch-content.js';

const DERIVED_FIELDS = [
  'block_number',
  'tx_hash',
  'tx_index',
  'mev_type',
  'extracted_value_wei',
  'sandwich_victim_tx',
  'ingested_at'
];

test('pitch is nine slides with a four-minute suggested duration', () => {
  assert.equal(slides.length, 9);
  assert.equal(PITCH_TOTAL_SECONDS, 240);
  assert.ok(slides.every((slide) => Number.isInteger(slide.duration) && slide.duration > 0));
});

test('pitch slide names, notes, visuals, and source references are complete', () => {
  assert.equal(new Set(slides.map((slide) => slide.short)).size, slides.length);
  for (const slide of slides) {
    assert.ok(slide.title.trim(), `${slide.short}: title`);
    assert.ok(slide.body.trim(), `${slide.short}: body`);
    assert.ok(slide.visual.trim(), `${slide.short}: visual`);
    assert.ok(slide.notes.trim(), `${slide.short}: speaker notes`);
    assert.ok(slide.status.trim(), `${slide.short}: evidence state`);
    assert.ok(slide.source.label.trim(), `${slide.short}: source label`);
    assert.match(slide.source.url, /^https:\/\//, `${slide.short}: source URL`);
  }
});

test('finding records have unique identities and six inspectable steps', () => {
  assert.equal(new Set(findings.map((finding) => finding.id)).size, findings.length);
  assert.equal(new Set(findings.map((finding) => finding.tx)).size, findings.length);
  for (const finding of findings) {
    assert.match(finding.tx, /^SYNTHETIC TX [A-D]$/);
    assert.match(finding.block, /^SAMPLE BLOCK /);
    assert.equal(finding.nodes.length, 6);
    assert.equal(finding.uniqueAddresses, 6);
    assert.ok(finding.nodes.every(([label, explanation]) => label.trim() && explanation.trim()));
  }
});

test('derived responses match the public MEV API field contract', () => {
  for (const finding of findings) {
    const response = derivedResponse(finding);
    assert.deepEqual(derivedResponsePayload(finding), [response]);
    assert.deepEqual(Object.keys(response), DERIVED_FIELDS);
    assert.equal(response.tx_hash, finding.tx);
    assert.equal(response.block_number, finding.blockNumber);
    assert.equal(response.tx_index, finding.txIndex);
    assert.equal(response.mev_type, finding.mevType);
    assert.match(response.extracted_value_wei, /SYNTHETIC ESTIMATE/);
    assert.match(response.ingested_at, /SYNTHETIC/);
    assert.equal(
      response.sandwich_victim_tx,
      finding.filter === 'sandwich' ? 'SYNTHETIC VICTIM CONTEXT' : ''
    );
  }
});

test('SGIP metadata preserves the same transaction identity and graph counts', () => {
  for (const finding of findings) {
    const metadata = sgipMetadata(finding);
    assert.deepEqual(Object.keys(metadata), ['tx_hash', 'total_traces', 'unique_addresses']);
    assert.equal(metadata.tx_hash, derivedResponse(finding).tx_hash);
    assert.equal(metadata.total_traces, finding.nodes.length);
    assert.equal(metadata.unique_addresses, finding.uniqueAddresses);
  }
});

test('candidate, synthetic, separate-endpoint, and planned boundaries stay visible', () => {
  const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
  const investigationSlide = slides.find((slide) => slide.short === 'Investigation');
  const roadmapSlide = slides.find((slide) => slide.short === 'Roadmap');
  const proofSlide = slides.find((slide) => slide.short === 'Proof');
  assert.match(investigationSlide.visual, /Candidate ≠ intent ≠ realized profit/);
  assert.match(investigationSlide.meta, /SYNTHETIC TX A/);
  assert.match(roadmapSlide.status, /PLANNED · NOT IMPLEMENTED/);
  assert.match(roadmapSlide.notes, /not the Arc Studio event frontend or partner evidence/);
  assert.match(proofSlide.visual, /Planned, not yet run/);
  assert.match(html, /not a composite production endpoint/);
  assert.match(html, /No payment, identity check, chain query or settlement is executed here/);
  assert.match(html, /SYNTHETIC PRODUCT PREVIEW/);
});
