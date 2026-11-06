import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { readFile } from 'node:fs/promises';
const execute = promisify(execFile);
const SYNTHETIC_SYSTEM = 'urn:phv:demo';
const isSynthetic = r => r.meta?.tag?.some(c => c.system === SYNTHETIC_SYSTEM && c.code === 'synthetic');
const ref = r => `${r.resourceType}/${r.id}`;
const codes = r => r.code?.coding ?? [];
const testCodes = new Map([['718-7', 'blood_count'], ['3016-3', 'tsh'], ['2339-0', 'glucose']]);

// Deliberately narrow mapping for the synthetic integration slice. No narrative HTML is executed.
export function adaptSnapshot(snapshot, template) {
  if (snapshot.schemaVersion !== 1 || !snapshot.id || !Array.isArray(snapshot.records)) throw new Error('Unsupported vault snapshot.');
  const records = snapshot.records;
  const ids = records.map(x => ref(x.resource));
  if (new Set(ids).size !== ids.length || records.some(x => !x.resource.id)) throw new Error('Duplicate or missing record identity.');
  const patients = records.filter(x => x.resource.resourceType === 'Patient');
  if (patients.length !== 1 || !isSynthetic(patients[0].resource)) throw new Error('This preview requires exactly one explicitly synthetic Patient.');
  const patient = patients[0].resource, patientRef = ref(patient);
  if (!patient.address?.some(a => a.country === 'PT') || !/^\d{4}-\d{2}-\d{2}$/.test(patient.birthDate ?? '')) throw new Error('The synthetic patient needs a birth date and Portugal country context.');
  const captureDate = new Date(snapshot.capturedAt);
  const birth = new Date(patient.birthDate);
  const ageYears = captureDate.getUTCFullYear() - birth.getUTCFullYear() - (captureDate.toISOString().slice(5, 10) < patient.birthDate.slice(5) ? 1 : 0);
  if (!Number.isFinite(ageYears) || ageYears < 18 || birth.toISOString().slice(0, 10) !== patient.birthDate) throw new Error('This preview supports synthetic adults only.');
  const byRef = new Map(records.map(x => [ref(x.resource), x]));
  const evidence = [], omitted = [];
  for (const row of records) {
    const r = row.resource, resourceRef = ref(r);
    if (r.resourceType === 'DocumentReference' && r.subject?.reference && r.subject.reference !== patientRef) throw new Error('A source document belongs to another patient.');
    if (['Patient', 'DocumentReference', 'Provenance'].includes(r.resourceType)) continue;
    if (!isSynthetic(r)) throw new Error('Every clinical record must be explicitly synthetic.');
    if (r.subject?.reference !== patientRef) throw new Error(`Patient binding is missing or mismatched for ${resourceRef}.`);
    let category, test, symptomCodeMatch = false, field, value;
    if (r.resourceType === 'Observation') {
      category = 'laboratory';
      test = codes(r).filter(c => c.system === 'http://loinc.org').map(c => testCodes.get(c.code)).find(Boolean) ?? 'unmapped';
      field = r.valueQuantity ? 'valueQuantity' : 'valueString';
      const q = r.valueQuantity;
      value = q ? `${q.value} ${q.unit ?? q.code ?? ''}` : r.valueString;
      if (!['final', 'amended', 'corrected'].includes(r.status)) { omitted.push({resourceRef, reason: `Observation status ${r.status} is not eligible.`}); continue; }
    } else if (r.resourceType === 'Condition') {
      category = 'symptom';
      symptomCodeMatch = codes(r).some(c => c.system === 'http://snomed.info/sct' && c.code === '84229001');
      field = 'code'; value = r.code?.text;
      if (r.verificationStatus?.coding?.some(c => ['entered-in-error', 'refuted'].includes(c.code))) { omitted.push({resourceRef, reason: 'Condition is refuted or entered in error.'}); continue; }
    } else if (r.resourceType === 'MedicationStatement') {
      category = 'medication'; field = 'medicationCodeableConcept'; value = r.medicationCodeableConcept?.text;
      if (r.status === 'entered-in-error') { omitted.push({resourceRef, reason: 'Medication statement is entered in error.'}); continue; }
    } else { omitted.push({resourceRef, reason: 'Resource type is outside this adapter mapping.'}); continue; }
    if (value === undefined || value === null || String(value).trim() === '') { omitted.push({resourceRef, reason: 'Supported structured value is missing.'}); continue; }
    const title = r.code?.text ?? r.medicationCodeableConcept?.text ?? r.resourceType;
    const date = r.effectiveDateTime ?? r.onsetDateTime ?? r.dateAsserted ?? 'Date not recorded';
    const documentRefs = records.filter(x => x.resource.resourceType === 'Provenance' && x.resource.target?.some(t => t.reference === resourceRef))
      .flatMap(x => x.resource.entity ?? []).map(e => e.what?.reference).filter(Boolean);
    const documents = [...new Set(documentRefs)].map(reference => {
      const document = byRef.get(reference)?.resource;
      if (!document || document.resourceType !== 'DocumentReference') throw new Error(`Missing provenance document for ${resourceRef}.`);
      const url = document.content?.[0]?.attachment?.url, source = snapshot.sources[url];
      if (!source) throw new Error(`Missing source for ${resourceRef}.`);
      return {reference, locator: url, ...source};
    });
    const notes = (r.note ?? []).map(n => n.text).filter(n => n && n !== value);
    const description = value === title ? title : `${title}: ${value}`;
    evidence.push({id: resourceRef, resourceRef, patientRef, title, text: description + (notes.length ? `\nRecord note: ${notes.join(' ')}` : ''), date,
      category, test, symptomCodeMatch, reviewStatus: 'unreviewed', recordHash: row.sha256,
      source: {title: documents.length ? 'Imported synthetic source and structured record' : 'Structured synthetic vault record (no document provenance)', locator: `${row.locator} ${resourceRef}.${field}`, documents}});
  }
  if (!evidence.length) throw new Error('No supported clinical evidence was found in the synthetic vault.');
  return {schemaVersion: 1, synthetic: true, id: snapshot.id,
    patient: {id: patientRef, label: patient.name?.[0]?.text ?? 'Synthetic person', ageYears, jurisdiction: 'PT'}, framework: structuredClone(template.framework),
    coverage: `${evidence.length} imported evidence items; ${omitted.length} clinical records outside the supported mapping or eligibility rules. Partial synthetic history.`, evidence,
    vaultSnapshot: {id: snapshot.id, capturedAt: snapshot.capturedAt, adapterVersion: snapshot.adapterVersion, mappingVersion: 'fatigue-vault-1', vaultBinding: snapshot.vaultBinding, patientRef, files: snapshot.files, omitted}};
}

export async function readVaultSnapshot({vaultPath, phvBinary = 'phv'}) {
  if (!vaultPath) throw new Error('No synthetic vault has been configured for this preview.');
  const {stdout} = await execute(phvBinary, ['--vault', vaultPath, '--json', 'snapshot'], {timeout: 8000, maxBuffer: 80 * 1024 * 1024});
  const template = JSON.parse(await readFile(new URL('./fixture.json', import.meta.url), 'utf8'));
  return adaptSnapshot(JSON.parse(stdout), template);
}
