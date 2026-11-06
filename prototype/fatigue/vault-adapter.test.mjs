import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile, readdir, stat, mkdtemp, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {once} from 'node:events';
import {adaptSnapshot, readVaultSnapshot} from './vault-adapter.mjs';
import {createSyntheticVault} from './create-synthetic-vault.mjs';
import {evaluateCase} from './engine.mjs';
import {createPreviewServer} from './server.mjs';
const execute = promisify(execFile);
const template = JSON.parse(await readFile(new URL('./fixture.json', import.meta.url), 'utf8'));
const tag = {tag: [{system: 'urn:phv:demo', code: 'synthetic'}]};
function raw() {
  return {schemaVersion: 1, id: 'vault-test', adapterVersion: 'phv-snapshot-1', vaultBinding: 'test-binding', capturedAt: '2026-11-06T12:00:00Z', files: {}, sources: {}, records: [
    {resource: {resourceType: 'Patient', id: 'p', meta: tag, birthDate: '1984-05-12', address: [{country: 'PT'}]}, sha256: 'patient-hash', locator: 'records/Patient.ndjson:1'},
    {resource: {resourceType: 'Observation', id: 'o', meta: tag, status: 'final', subject: {reference: 'Patient/p'}, code: {coding: [{system: 'http://loinc.org', code: '718-7'}], text: 'Hemoglobin'}, valueQuantity: {value: 13.6, unit: 'g/dL'}, effectiveDateTime: '2026-10-16'}, sha256: 'record-hash', locator: 'records/Observation.ndjson:1'}]};
}
test('vault evidence is patient-bound, unreviewed by default and outside the brief until reviewed', async () => {
  const fixture = adaptSnapshot(raw(), template);
  const input = {concern: 'Synthetic tiredness'};
  const deferred = await evaluateCase(fixture, input);
  assert.equal(deferred.decisions[0].disposition, 'defer-for-confirmation');
  assert.equal(deferred.brief.claims.filter(c => c.attribution === 'source_fact').length, 0);
  fixture.evidence[0].reviewStatus = 'reviewed';
  const reviewed = await evaluateCase(fixture, input);
  assert.equal(reviewed.decisions[0].disposition, 'include');
  assert.equal(reviewed.trace.recordReviews[0].recordHash, 'record-hash');
  assert.equal(reviewed.trace.vaultSnapshot.patientRef, 'Patient/p');
  assert.equal(reviewed.evidence[0].source.documents.length, 0);
});
test('adapter rejects real, mixed-patient, duplicate and unsupported patient contexts', () => {
  for (const mutate of [s => {s.records[0].resource.meta = {};}, s => {s.records[1].resource.meta = {};}, s => {s.records[1].resource.subject.reference = 'Patient/other';}, s => s.records.push(s.records[1]), s => {s.records[0].resource.birthDate = '2020-01-01';}, s => {s.records[0].resource.address[0].country = 'DE';}]) {
    const snapshot = raw(); mutate(snapshot); assert.throws(() => adaptSnapshot(snapshot, template));
  }
});
test('entered-in-error data is accounted for and never becomes evidence', () => {
  const snapshot = raw();
  snapshot.records.push({...structuredClone(snapshot.records[1]), resource: {...structuredClone(snapshot.records[1].resource), id: 'bad', status: 'entered-in-error'}});
  const fixture = adaptSnapshot(snapshot, template);
  assert.equal(fixture.evidence.length, 1); assert.equal(fixture.vaultSnapshot.omitted.length, 1);
});
test('missing provenance source fails explicitly; it is never fabricated', () => {
  const snapshot = raw();
  snapshot.records.push({resource: {resourceType: 'Provenance', id: 'prov', target: [{reference: 'Observation/o'}], entity: [{what: {reference: 'DocumentReference/missing'}}]}});
  assert.throws(() => adaptSnapshot(snapshot, template), /Missing provenance/);
});
async function tree(path) {
  const out = {};
  for (const name of (await readdir(path)).sort()) {
    const target = join(path, name), metadata = await stat(target);
    out[name] = metadata.isDirectory() ? await tree(target) : {content: (await readFile(target)).toString('hex'), mtime: metadata.mtimeMs, mode: metadata.mode};
  }
  return out;
}
test('real validated import → read-only snapshot → review → brief → correction; HTTP and stale capture', {skip: !process.env.PHV_TEST_BINARY}, async () => {
  const folder = await mkdtemp(join(tmpdir(), 'phv-flow-test-'));
  let server;
  try {
    const vaultPath = join(folder, 'vault'), phvBinary = process.env.PHV_TEST_BINARY;
    const imported = await createSyntheticVault(vaultPath, phvBinary);
    assert.deepEqual(imported.issues, []);
    const before = await tree(vaultPath);
    const fixture = await readVaultSnapshot({vaultPath, phvBinary});
    assert.equal(fixture.evidence.length, 8);
    assert.ok(fixture.evidence.every(e => e.reviewStatus === 'unreviewed' && e.source.documents[0].sha256 === imported.sourceHash));
    const input = {concern: 'I am tired; prepare a synthetic brief.', answers: {onset: {status: 'answered', value: 'Around September'}, impact: {status: 'answered', value: 'More breaks'}, medication_current: {status: 'unknown'}, goal: {status: 'skipped'}}};
    const deferred = await evaluateCase(fixture, input);
    assert.ok(deferred.decisions.every(d => d.disposition === 'defer-for-confirmation'));
    // Keep the sleep extraction unchecked; other records are reviewed against the pinned source.
    fixture.evidence.forEach(e => {if (!e.title.includes('sleep')) e.reviewStatus = 'reviewed';});
    const brief = await evaluateCase(fixture, input);
    assert.equal(brief.decisions.filter(d => d.disposition === 'include').length, 6);
    assert.equal(brief.decisions.filter(d => d.disposition === 'exclude').length, 1);
    assert.equal(brief.decisions.filter(d => d.disposition === 'defer-for-confirmation').length, 1);
    const claim = brief.brief.claims.find(c => c.evidenceIds.some(id => fixture.evidence.find(e => e.id === id)?.test === 'blood_count'));
    const corrected = await evaluateCase(fixture, {...input, revision: 2, claimEdits: {[claim.id]: 'I want the doctor to verify this historical result.'}});
    const correction = corrected.brief.claims.find(c => c.id === claim.id);
    assert.equal(correction.attribution, 'patient_report'); assert.equal(correction.originalText, claim.text);
    assert.deepEqual(corrected.evidence, brief.evidence); assert.equal(corrected.snapshot.vault.id, fixture.id);
    assert.deepEqual(await tree(vaultPath), before);
    server = createPreviewServer({vaultPath, phvBinary}); server.listen(0, '127.0.0.1'); await once(server, 'listening');
    const base = `http://127.0.0.1:${server.address().port}`;
    assert.equal((await (await fetch(base + '/preview-config')).json()).vaultEnabled, true);
    const first = await (await fetch(base + '/vault-snapshot')).json();
    assert.equal(first.id, fixture.id);
    assert.equal((await fetch(base + '/vault-snapshot', {method: 'POST'})).status, 405);
    assert.equal((await fetch(base + '/vault-snapshot', {headers: {Origin: 'https://example.invalid'}})).status, 403);
    assert.equal((await fetch(base + '/vault-snapshot?path=/etc/passwd')).status, 200); // Query cannot select a different vault.
    assert.deepEqual(await tree(vaultPath), before);
    await execute(phvBinary, ['--vault', vaultPath, 'source', 'add', new URL('./fixture.json', import.meta.url).pathname]);
    const changed = await (await fetch(base + '/vault-snapshot')).json();
    assert.notEqual(changed.id, fixture.id); assert.deepEqual(corrected.evidence, brief.evidence);
  } finally {
    if (server) {server.closeAllConnections(); await new Promise(resolve => server.close(resolve));}
    await rm(folder, {recursive: true, force: true});
  }
});
