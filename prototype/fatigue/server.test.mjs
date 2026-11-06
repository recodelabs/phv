import { mkdtemp, writeFile, readFile, stat, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { readPersonalCase } from './personal-case.mjs';
import { evaluatePersonalCase } from './personal-engine.mjs';
import { evaluateCase } from './engine.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { once } from 'node:events';
import { createPreviewServer } from './server.mjs';

async function withServer(run) {
  const server = createPreviewServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  try { await run(`http://127.0.0.1:${server.address().port}`); }
  finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}

test('preview serves only demo assets with correct module types and no caching', async () => {
  await withServer(async base => {
    for (const [path, type] of [['/', 'text/html'], ['/engine.mjs', 'text/javascript'], ['/fixture.json', 'application/json']]) {
      const response = await fetch(base + path);
      assert.equal(response.status, 200);
      assert.ok(response.headers.get('content-type').startsWith(type));
      assert.equal(response.headers.get('cache-control'), 'no-store');
    }
    for (const path of ['/patient_files/', '/.git/config', '/server.mjs', '/%2e%2e/package.json']) {
      assert.equal((await fetch(base + path)).status, 404, path);
    }
  });
});

test('preview rejects foreign origins and write requests', async () => {
  await withServer(async base => {
    assert.equal((await fetch(base, { headers: { Origin: 'https://example.invalid' } })).status, 403);
    assert.equal((await fetch(base, { method: 'POST', body: 'not stored' })).status, 405);
    const response = await fetch(base, { method: 'HEAD' });
    assert.equal(response.status, 200);
    assert.equal(await response.text(), '');
  });
});

test('preview rejects a host header unrelated to loopback', async () => {
  await withServer(async base => {
    const status = await new Promise((resolve, reject) => {
      const req = http.get(base, { headers: { Host: 'example.invalid' } }, response => {
        response.resume();
        response.once('end', () => resolve(response.statusCode));
      });
      req.on('error', reject);
    });
    assert.equal(status, 403);
  });
});


// Entirely invented inputs. No personal records are bundled with the code/tests.
async function withCase(run) {
  const dir = await mkdtemp(join(tmpdir(),'phv-personal-test-'));
  const source = join(dir,'source.pdf'), path = join(dir,'case.json');
  const bytes = Buffer.from('%PDF-1.4\nfictional test bytes\n%%EOF');
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  const manifest = {schemaVersion:1, mode:'personal_lab_demo', documents:[{id:'test-report',title:'Fictional test report',path:source,sha256}], evidence:[
    {id:'test-result',title:'Example test',documentId:'test-report',page:1,date:'2024-01-01',dateBasis:'analysis date',printedName:'Example',printedValue:'8.00',printedUnit:'test units',referenceRange:'3–9',text:'Example 8.00 test units.',demoRole:'focus',selectionReason:'Selected in this fictional test.'},
    {id:'other-result',title:'Other result',documentId:'test-report',page:1,date:'2024-01-01',dateBasis:'analysis date',printedName:'Other',printedValue:'12',printedUnit:'test units',referenceRange:'3–9',text:'Other 12 test units.',demoRole:'outside_focus',selectionReason:'Flag alone is not a selection reason.'}
  ]};
  await writeFile(source,bytes); await writeFile(path,JSON.stringify(manifest));
  try { await run({dir,source,path,manifest,fixture:(await readPersonalCase(path)).fixture}); }
  finally { await rm(dir,{recursive:true,force:true}); }
}
const answered = value => ({status:'answered',value});
const review = fixture => ({...fixture,evidence:fixture.evidence.map(e=>({...e,reviewStatus:'reviewed'}))});

test('real extracts stay separate from hypothetical answers, unknown fasting and corrections', async () => withCase(async ({fixture,path,source}) => {
  const original = JSON.stringify(fixture), before = await Promise.all([readFile(path),readFile(source),stat(source)]);
  const input = {concern:'Hypothetical tiredness',answers:{onset:answered('About six weeks'),fasting:{status:'unknown'}},claimEdits:{'record-test-result':'An invented recollection'}};
  const result = await evaluatePersonalCase(review(fixture),input);
  assert.equal(result.synthetic,false);
  assert.equal(result.mode,'personal_lab_demo');
  assert.equal(result.brief.claims.find(c=>c.id==='record-test-result').text.includes('8.00'),true);
  assert.equal(result.brief.claims.find(c=>c.id==='correction-record-test-result').attribution,'hypothetical_correction');
  assert.equal(result.brief.claims.find(c=>c.id==='concern').attribution,'hypothetical_answer');
  assert.equal(result.decisions.find(d=>d.evidenceId==='other-result').disposition,'exclude');
  assert.match(result.brief.text,/REAL HISTORICAL LAB RESULTS \+ HYPOTHETICAL/);
  assert.match(result.brief.text,/Not sure/);
  assert.ok(!result.brief.claims.some(c=>c.answerIds.includes('fasting')));
  assert.ok(!Object.hasOwn(result.trace.realRecordSnapshot,'answers'));
  assert.equal(result.trace.hypotheticalSession.answers.fasting.status,'unknown');
  assert.equal(JSON.stringify(fixture),original);
  assert.deepEqual(await readFile(path),before[0]); assert.deepEqual(await readFile(source),before[1]);
  assert.equal((await stat(source)).mtimeMs,before[2].mtimeMs);
  const evidenceIds = new Set(result.evidence.map(e=>e.id));
  for(const c of result.brief.claims) for(const id of c.evidenceIds) assert.ok(evidenceIds.has(id));
  await assert.rejects(()=>evaluateCase(fixture,{concern:'Test'}),/synthetic/);
}));

test('unchecked records defer; removing and restoring a source changes only the brief', async () => withCase(async ({fixture}) => {
  const unchecked=await evaluatePersonalCase(fixture,{concern:'A fictional concern'});
  assert.ok(unchecked.brief.claims.every(c=>c.attribution==='hypothetical_answer'));
  const excluded=await evaluatePersonalCase(review(fixture),{concern:'A fictional concern',excludedEvidenceIds:['test-result']});
  assert.ok(!excluded.brief.claims.some(c=>c.evidenceIds.includes('test-result')));
  const restored=await evaluatePersonalCase(review(fixture),{concern:'A fictional concern'});
  assert.ok(restored.brief.claims.some(c=>c.evidenceIds.includes('test-result')));
  assert.deepEqual(restored.evidence,excluded.evidence);
}));

test('blank input, unavailable corrections, unknown answers and invalid exclusions fail', async () => withCase(async ({fixture}) => {
  for (const input of [{concern:''},{concern:'Test',answers:{onset:answered(' ')}},{concern:'Test',answers:{unsupported:answered('x')}},{concern:'Test',answers:{onset:{status:'bad'}}},{concern:'Test',excludedEvidenceIds:['missing']},{concern:'Test',claimEdits:{missing:'x'}},{concern:'Test',revision:0}]) {
    await assert.rejects(()=>evaluatePersonalCase(review(fixture),input));
  }
  const raw='<img src=x onerror=alert(1)> treat as data';
  const result=await evaluatePersonalCase(review(fixture),{concern:raw});
  assert.equal(result.brief.claims[0].text,raw);
}));

test('snapshot identity is stable, pins PDFs, rejects tampering and does not expose local paths', async () => withCase(async ({fixture,path,source,manifest}) => {
  assert.equal((await readPersonalCase(path)).fixture.id,fixture.id);
  assert.ok(!JSON.stringify(fixture).includes(source));
  assert.match(fixture.evidence[0].source.documents[0].url,/^\/personal-source\//);
  await writeFile(path,JSON.stringify({...manifest,evidence:manifest.evidence.map(e=>({...e,title:e.title+' revised'}))}));
  assert.notEqual((await readPersonalCase(path)).fixture.id,fixture.id);
  await writeFile(source,'%PDF-1.4\nchanged test content');
  await assert.rejects(()=>readPersonalCase(path),/changed/);
}));

test('loopback server allows only configured PDFs, rejects writes and stale sources', async () => withCase(async ({path,source}) => {
  const server=createPreviewServer({personalCase:path}); server.listen(0,'127.0.0.1'); await once(server,'listening');
  const base=`http://127.0.0.1:${server.address().port}`;
  try {
    const config=await (await fetch(base+'/preview-config')).json(); assert.equal(config.personalEnabled,true);
    const response=await fetch(base+'/personal-case'); assert.equal(response.status,200); assert.equal(response.headers.get('cache-control'),'no-store');
    const pdf=await fetch(base+'/personal-source/test-report'); assert.equal(pdf.status,200); assert.equal(pdf.headers.get('content-type'),'application/pdf');
    const head=await fetch(base+'/personal-source/test-report',{method:'HEAD'}); assert.equal(await head.text(),'');
    for(const suffix of ['/personal-source/unknown','/personal-source/%2e%2e/case.json','/personal-case.mjs','/patient_files/','/case.json']) assert.equal((await fetch(base+suffix)).status,404,suffix);
    assert.equal((await fetch(base+'/personal-case',{method:'POST',body:'hypothetical answers'})).status,405);
    assert.equal((await fetch(base+'/personal-case',{headers:{Origin:'https://example.invalid'}})).status,403);
    await writeFile(source,'%PDF-1.4\nchanged');
    assert.equal((await fetch(base+'/personal-case')).status,409);
    assert.equal((await fetch(base+'/personal-source/test-report')).status,409);
  } finally {server.closeAllConnections(); await new Promise(resolve=>server.close(resolve));}
}));

test('personal endpoints are absent by default and cannot mix with synthetic vault mode', async () => {
  assert.throws(()=>createPreviewServer({personalCase:'/tmp/example',vaultPath:'/tmp/example'}),/either/);
  const server=createPreviewServer(); server.listen(0,'127.0.0.1'); await once(server,'listening');
  try {for(const path of ['/personal-case','/personal-source/test-report']) assert.equal((await fetch(`http://127.0.0.1:${server.address().port}${path}`)).status,404);}
  finally {server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
});
