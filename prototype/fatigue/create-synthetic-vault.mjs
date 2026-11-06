// Imports only generated fictional data through phv's validated write path.
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdtemp, readFile, writeFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
const execute = promisify(execFile);
export async function createSyntheticVault(vaultPath, phvBinary = 'phv') {
  const working = await mkdtemp(join(tmpdir(), 'phv-synthetic-import-'));
  const run = async args => JSON.parse((await execute(phvBinary, ['--json', ...args], {maxBuffer: 8 * 1024 * 1024})).stdout);
  try {
    const fixture = JSON.parse(await readFile(new URL('./fixture.json', import.meta.url), 'utf8'));
    const meta = {tag: [{system: 'urn:phv:demo', code: 'synthetic'}]};
    await run(['init', resolve(vaultPath)]);
    const patientFile = join(working, 'patient.json');
    await writeFile(patientFile, JSON.stringify({resourceType: 'Patient', meta, name: [{text: 'Synthetic Example Person'}], birthDate: '1984-05-12', address: [{country: 'PT'}]}));
    const patient = (await run(['--vault', vaultPath, 'add', patientFile])).created[0];
    const sourceFile = join(working, 'synthetic-fatigue-source.txt');
    await writeFile(sourceFile, 'FICTIONAL RECORDS FOR SOFTWARE TESTING\n\n' + fixture.evidence.map(e => `${e.id} | ${e.date} | ${e.title}\n${e.text}`).join('\n\n'));
    const source = await run(['--vault', vaultPath, 'source', 'add', sourceFile]);
    const labValues = {'lab-cbc': ['718-7', 13.6, 'g/dL'], 'lab-tsh': ['3016-3', 2.1, 'mIU/L'], 'lab-glucose': ['2339-0', 91, 'mg/dL']};
    const resources = fixture.evidence.map(e => {
      const common = {meta, subject: {reference: patient}, note: [{text: e.text}]};
      if (e.category === 'symptom') return {...common, resourceType: 'Condition', code: {coding: [{system: 'http://snomed.info/sct', code: '84229001'}], text: e.title}, onsetDateTime: e.date};
      if (e.category === 'medication') return {...common, resourceType: 'MedicationStatement', status: 'unknown', medicationCodeableConcept: {text: 'Example medicine A'}, dateAsserted: e.date};
      const lab = labValues[e.id];
      return {...common, resourceType: 'Observation', status: 'final', code: {coding: [{system: lab ? 'http://loinc.org' : 'urn:phv:demo:observations', code: lab?.[0] ?? e.id}], text: e.title}, effectiveDateTime: e.date,
        ...(lab ? {valueQuantity: {value: lab[1], unit: lab[2]}} : {valueString: e.text})};
    });
    const bundleFile = join(working, 'records.json');
    await writeFile(bundleFile, JSON.stringify({resourceType: 'Bundle', type: 'collection', entry: resources.map(resource => ({resource}))}));
    const report = await run(['--vault', vaultPath, 'add', bundleFile, '--source', source.sha256, '--agent', 'synthetic-fatigue-import', '--recorded-at', '2026-11-06T12:00:00Z']);
    const issues = await run(['--vault', vaultPath, 'check']);
    if (issues.some(issue => issue.severity === 'error')) throw new Error('Synthetic import failed vault consistency checks.');
    return {vaultPath: resolve(vaultPath), patient, sourceHash: source.sha256, created: report.created, issues};
  } finally { await rm(working, {recursive: true, force: true}); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = process.argv.slice(2);
    if (!args[0] || args.length > 2) throw new Error('Usage: node create-synthetic-vault.mjs NEW_VAULT_PATH [PHV_BINARY]');
    console.log(JSON.stringify(await createSyntheticVault(args[0], args[1]), null, 2));
  } catch (error) { console.error(error.stderr ?? error.message); process.exitCode = 1; }
}
