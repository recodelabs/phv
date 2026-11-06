// Server-only reader. A private manifest explicitly allowlists the source PDFs.
import { readFile, realpath, stat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { isAbsolute, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const repo = fileURLToPath(new URL('../../', import.meta.url));
const inside = (root, path) => { const r = relative(root, path); return r === '' || (!r.startsWith('..') && !isAbsolute(r)); };
const idOK = id => typeof id === 'string' && /^[a-z0-9][a-z0-9-]{0,79}$/.test(id);
const requireValue = (ok, message) => { if (!ok) throw new Error(message); };

export async function readPersonalCase(manifestPath) {
  requireValue(manifestPath, 'No personal demo was configured.');
  const path = await realpath(resolve(manifestPath));
  requireValue(!inside(await realpath(repo), path), 'Keep the private manifest outside the code repository.');
  requireValue((await stat(path)).size <= 1024 * 1024, 'Personal manifest is too large.');
  const raw = await readFile(path), manifest = JSON.parse(raw);
  requireValue(manifest.schemaVersion === 1 && manifest.mode === 'personal_lab_demo', 'Unsupported personal demo manifest.');
  requireValue(Array.isArray(manifest.documents) && manifest.documents.length > 0 && manifest.documents.length <= 12, 'Configure 1–12 source documents.');
  requireValue(Array.isArray(manifest.evidence) && manifest.evidence.length <= 100, 'Configure at most 100 selected results.');
  const files = new Map(), documents = new Map();
  let total = 0;
  for (const doc of manifest.documents) {
    requireValue(idOK(doc.id) && !documents.has(doc.id) && typeof doc.title === 'string' && isAbsolute(doc.path), 'Invalid or duplicate source document.');
    const sourcePath = await realpath(doc.path);
    const size = (await stat(sourcePath)).size;
    total += size;
    requireValue(size <= 16 * 1024 * 1024 && total <= 64 * 1024 * 1024, 'Source documents exceed the preview limits.');
    const bytes = await readFile(sourcePath);
    requireValue(bytes.subarray(0, 5).toString() === '%PDF-', 'Source must be a PDF.');
    const sha256 = digest(bytes);
    requireValue(sha256 === doc.sha256, 'A source PDF changed; review the manifest before continuing.');
    const publicDoc = {id: doc.id, title: doc.title, sha256, url: `/personal-source/${doc.id}`, locator: doc.dateNote ?? ''};
    documents.set(doc.id, publicDoc);
    files.set(doc.id, bytes);
  }
  const ids = new Set();
  const evidence = manifest.evidence.map(item => {
    requireValue(idOK(item.id) && !ids.has(item.id), 'Invalid or duplicate evidence ID.'); ids.add(item.id);
    const doc = documents.get(item.documentId);
    requireValue(doc && Number.isInteger(item.page) && item.page >= 1 && item.page <= 100, 'Missing source document or page.');
    for (const key of ['title', 'text', 'date', 'dateBasis', 'printedName', 'printedValue', 'printedUnit', 'referenceRange', 'selectionReason']) {
      requireValue(typeof item[key] === 'string' && item[key].length <= 5000, `Missing result field: ${key}`);
    }
    requireValue(['focus', 'context', 'outside_focus'].includes(item.demoRole), 'Missing demo selection role.');
    return {id: item.id, title: item.title, text: item.text, date: item.date, dateBasis: item.dateBasis,
      printedName: item.printedName, printedValue: item.printedValue, printedUnit: item.printedUnit, referenceRange: item.referenceRange,
      demoRole: item.demoRole, selectionReason: item.selectionReason, category: 'laboratory', reviewStatus: 'unreviewed',
      source: {title: doc.title, locator: `PDF page ${item.page} · ${item.printedName}`, documents: [{...doc, url: `${doc.url}#page=${item.page}`}]} };
  });
  const id = `personal-${digest(Buffer.concat([raw, ...[...files.values()]]))}`;
  return {files, fixture: {schemaVersion: 1, mode: 'personal_lab_demo', synthetic: false, id,
    patient: {label: 'My lab history'},
    framework: {id: 'personal-lab-demo', version: '1', status: 'draft', sources: []},
    coverage: `${evidence.length} selected results from ${documents.size} real reports. A curated extract, not a complete health record. Dates retain the labels printed in each report.`,
    evidence, personalSnapshot: {id, sourceHashes: [...documents.values()].map(d => ({id: d.id, sha256: d.sha256}))}}};
}
