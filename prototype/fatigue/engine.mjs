/** Deterministic, synthetic-only demonstration. Not a clinical rule evaluator. */
const QUESTION_IDS = ['onset', 'impact', 'medication_current', 'goal'];
const ANSWER_STATUSES = new Set(['answered', 'unknown', 'skipped', 'declined']);
const DEGAM = 'https://register.awmf.org/assets/guidelines/053-002l_S3_Muedigkeit_2023-01_01.pdf';
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = value => typeof value === 'string' && value.trim().length > 0;
const clone = value => JSON.parse(JSON.stringify(value));
function assert(condition, message) { if (!condition) throw new Error(message); }
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (object(value)) return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  return JSON.stringify(value);
}
async function hash(value) {
  const bytes = new TextEncoder().encode(canonical(value));
  const digest = await globalThis.crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('');
}
function citation(recommendation, page) {
  return {sourceId: 'DE-DEGAM-053002', label: `DEGAM S3 Müdigkeit v5.0, recommendation ${recommendation}, PDF p${page}`, url: `${DEGAM}#page=${page}`, support: 'adapted'};
}
const symptomCitation = () => citation('5.1.1', 12);
const medicationCitation = () => citation('5.1.4', 13);
const labCitation = () => citation('5.3.1', 15);

function validate(fixture, input) {
  assert(object(fixture) && fixture.synthetic === true, 'Only explicitly synthetic fixtures can be evaluated.');
  assert(fixture.schemaVersion === 1, 'Unsupported synthetic fixture schema version.');
  assert(text(fixture.id), 'The synthetic fixture needs an ID.');
  assert(object(fixture.patient) && Number.isFinite(fixture.patient.ageYears) && fixture.patient.ageYears >= 18 && fixture.patient.jurisdiction === 'PT', 'This demonstration supports synthetic adults in Portugal only.');
  assert(object(fixture.framework) && text(fixture.framework.id) && text(fixture.framework.version) && Array.isArray(fixture.framework.sources), 'Framework metadata and sources are required.');
  const sourceIds = fixture.framework.sources.map(source => source?.id);
  assert(sourceIds.includes('DE-DEGAM-053002') && new Set(sourceIds).size === sourceIds.length, 'Framework source IDs must be unique and include the referenced DEGAM edition.');
  assert(text(fixture.coverage) && Array.isArray(fixture.evidence), 'Evidence and coverage are required.');
  const ids = new Set();
  for (const item of fixture.evidence) {
    assert(object(item) && text(item.id) && !ids.has(item.id), 'Evidence IDs must be present and unique.');
    ids.add(item.id);
    assert(text(item.title) && text(item.text) && text(item.date) && text(item.category) && object(item.source) && text(item.source.title) && text(item.source.locator), `Evidence ${item.id} needs text, date, category and a source locator.`);
    assert(['reviewed', 'unreviewed'].includes(item.reviewStatus), `Unknown review state for ${item.id}.`);
  }
  assert(object(input), 'Input must be an object.');
  assert(text(input.concern), 'Describe your concern before continuing.');
  assert(input.concern.length <= 10000, 'Keep the concern under 10,000 characters.');
  const answers = input.answers ?? {};
  assert(object(answers), 'Answers must be an object.');
  for (const [id, answer] of Object.entries(answers)) {
    assert(QUESTION_IDS.includes(id), `Unknown question: ${id}.`);
    assert(object(answer) && ANSWER_STATUSES.has(answer.status), `Invalid answer status for ${id}.`);
    assert(answer.value === undefined || typeof answer.value === 'string', `Answer ${id} must contain text.`);
    if (answer.status === 'answered') assert(text(answer.value), `Enter an answer for ${id}, or choose Not sure or Skip.`);
    assert((answer.value ?? '').length <= 10000, `Keep the ${id} answer under 10,000 characters.`);
  }
  assert(input.excludedEvidenceIds === undefined || Array.isArray(input.excludedEvidenceIds), 'Excluded evidence IDs must be an array.');
  for (const id of input.excludedEvidenceIds ?? []) assert(ids.has(id), `Unknown excluded evidence: ${id}.`);
  assert(input.claimEdits === undefined || object(input.claimEdits), 'Claim edits must be an object.');
  for (const [id, edit] of Object.entries(input.claimEdits ?? {})) assert(text(edit) && edit.length <= 10000, `Claim edit ${id} must contain text under 10,000 characters.`);
  assert(input.revision === undefined || (Number.isSafeInteger(input.revision) && input.revision >= 1), 'Revision must be a positive integer.');
}

function decide(item, excluded) {
  const base = {id: `decision-${item.id}`, evidenceId: item.id, disposition: 'exclude', basis: 'demonstration_rule', ruleId: null, rationale: '', citation: null};
  if (excluded.has(item.id)) return {...base, disposition: 'exclude', basis: 'user_request', rationale: 'Removed from this brief by the person. The original synthetic source remains available.'};
  if (item.reviewStatus !== 'reviewed') return {...base, disposition: 'defer-for-confirmation', basis: 'record_review_policy', rationale: 'The extraction has not been checked against its source; it is kept outside the brief.'};
  if (item.category === 'symptom' && item.linkedToConcern === true) return {...base, disposition: 'include', basis: 'explicit_record_connection', ruleId: 'FAT-R001', rationale: 'The synthetic record explicitly connects this symptom history to the concern. An older linked episode is retained without claiming a shared cause.', citation: symptomCitation()};
  if (item.category === 'symptom' && item.symptomCodeMatch === true) return {...base, disposition: 'include', basis: 'guideline_framework', ruleId: 'FAT-R001', rationale: 'A fatigue code makes this a candidate under the draft symptom-history mapping. The code does not establish a documented connection to the current episode or a shared cause.', citation: symptomCitation()};
  if (item.category === 'medication') return {...base, disposition: 'include', basis: 'guideline_framework', ruleId: 'FAT-R003', rationale: 'Medication context is selected by a draft adaptation. This dated list does not establish current use or a cause of fatigue.', citation: medicationCitation()};
  if (item.category === 'laboratory' && ['blood_count', 'tsh', 'glucose', 'esr', 'crp', 'transaminases', 'ggt'].includes(item.test)) return {...base, disposition: 'include', basis: 'guideline_framework', ruleId: 'FAT-R007', rationale: 'An existing result is selected by the draft fatigue evidence mapping. This is not a recommendation for testing or an explanation of the symptom.', citation: labCitation()};
  return {...base, rationale: 'No explicit symptom link or applicable demonstration retrieval category was found. A printed abnormal flag alone does not establish relevance.'};
}

function buildQuestions(input, evidence, decisions) {
  const included = new Set(decisions.filter(d => d.disposition === 'include').map(d => d.evidenceId));
  const medicationIds = evidence.filter(e => e.category === 'medication' && included.has(e.id)).map(e => e.id);
  const questions = [
    {id: 'onset', text: 'When did this tiredness start? An approximate date is fine.', rationale: 'This places records before and during the episode while preserving uncertainty.', priority: 'essential', evidenceIds: evidence.filter(e => e.category === 'symptom' && included.has(e.id)).map(e => e.id), citation: symptomCitation(), basis: 'guideline_framework', ruleId: 'FAT-R001'},
    {id: 'impact', text: 'What has become harder to do in your usual day?', rationale: 'Your own description of daily impact helps the doctor understand the concern.', priority: 'essential', evidenceIds: [], citation: symptomCitation(), basis: 'guideline_framework', ruleId: 'FAT-R001'},
    ...(medicationIds.length || input.answers.medication_current ? [{id: 'medication_current', text: 'Are you still taking the medicine listed in the synthetic medication record? Note any changes, or say if you are unsure.', rationale: 'A dated medication list does not confirm what is currently taken. Your answer will be shown separately from that source.', priority: 'essential', evidenceIds: medicationIds, citation: medicationCitation(), basis: 'guideline_framework', ruleId: 'FAT-R003'}] : []),
    {id: 'goal', text: 'What would you most like to discuss with your doctor?', rationale: 'Your consultation goal helps organize the brief. No clinical guideline citation is claimed for this prompt.', priority: 'optional', evidenceIds: [], citation: null, basis: 'user_request', ruleId: null}
  ];
  return questions.map(q => ({...q, answerStatus: input.answers[q.id]?.status ?? 'unanswered', answer: input.answers[q.id] ?? null}));
}
function claim(id, section, value, attribution, evidenceIds = [], decisionIds = [], answerIds = []) {
  return {id, section, text: value, attribution, evidenceIds, decisionIds, answerIds};
}
function buildBrief(input, evidence, decisions, questions, snapshot, framework) {
  const claims = [claim('concern', 'concern', input.concern, 'patient_report', [], [], ['concern'])];
  const templates = {
    onset: ['onset', 'timeline', 'Reported onset: '],
    impact: ['impact', 'concern', 'Reported daily impact: '],
    medication_current: ['medication-current', 'history', 'Current medication account reported by the person: '],
    goal: ['goal', 'questions', 'The person wants to discuss: ']
  };
  for (const q of questions) {
    const answer = input.answers[q.id];
    if (answer?.status !== 'answered') continue;
    const [id, section, prefix] = templates[q.id];
    claims.push(claim(id, section, prefix + answer.value, 'patient_report', [], [], [q.id]));
  }
  const byId = new Map(evidence.map(e => [e.id, e]));
  const orderedDecisions = [...decisions].sort((a, b) => {
    const first = byId.get(a.evidenceId);
    const second = byId.get(b.evidenceId);
    if (first.category === 'symptom' && second.category === 'symptom') return first.date.localeCompare(second.date);
    if (first.category === 'symptom') return -1;
    if (second.category === 'symptom') return 1;
    return 0;
  });
  for (const decision of orderedDecisions) {
    if (decision.disposition !== 'include') continue;
    const item = byId.get(decision.evidenceId);
    claims.push(claim(`record-${item.id}`, item.category === 'symptom' ? 'timeline' : 'history', `${item.date}: ${item.text}`, 'source_fact', [item.id], [decision.id]));
  }
  const validIds = new Set(claims.map(c => c.id));
  for (const id of Object.keys(input.claimEdits)) assert(validIds.has(id), `Claim ${id} is unavailable in the current brief; restore its source before editing it.`);
  const editedClaims = claims.map(c => Object.hasOwn(input.claimEdits, c.id) ? {...c, originalText: c.text, text: input.claimEdits[c.id], attribution: 'patient_report', edited: true, correctionOfEvidenceIds: [...c.evidenceIds]} : c);
  const statusText = {unanswered: 'Not answered', unknown: 'The person is unsure', skipped: 'Skipped', declined: 'The person declined to answer'};
  const gaps = questions.filter(q => q.answerStatus !== 'answered').map(q => `${q.text} — ${statusText[q.answerStatus]}.`);
  if (decisions.some(d => d.disposition === 'defer-for-confirmation')) gaps.push('An unreviewed extraction was not included.');
  gaps.push('This short demonstration does not complete the broader fatigue assessment. Post-COVID selection, mental-health screening and urgency routing are not implemented.');
  const lines = ['SYNTHETIC DEMONSTRATION — DRAFT FRAMEWORK, NOT CLINICALLY APPROVED', 'Fatigue consultation brief', `Revision ${input.revision}`, `Coverage: ${snapshot.coverage}`, ''];
  for (const [section, heading] of [['concern','Concern'],['timeline','Timeline and documented symptom history'],['history','Relevant documented context'],['questions','Questions for the doctor']]) {
    lines.push(heading);
    const group = editedClaims.filter(c => c.section === section);
    if (!group.length) lines.push('Not yet supplied.');
    for (const c of group) lines.push(`- ${c.text} (${c.attribution === 'patient_report' ? 'Patient report' : 'Source fact'})${c.evidenceIds.map(id => ` [${id}]`).join('')}`);
    lines.push('');
  }
  lines.push('Gaps and uncertainty', ...gaps.map(g => `- ${g}`), '', 'Evidence index');
  for (const d of decisions.filter(d => d.disposition === 'include')) {
    const e = byId.get(d.evidenceId); lines.push(`[${e.id}] ${e.source.title}; ${e.source.locator}`);
  }
  lines.push('', `Framework: ${framework.id} ${framework.version} (draft; clinical review pending)`, `Snapshot: ${snapshot.id}`, 'No diagnosis, test order or treatment recommendation is generated.');
  return {revision: input.revision, claims: editedClaims, gaps, text: lines.join('\n')};
}

export async function evaluateCase(fixture, suppliedInput = {}) {
  validate(fixture, suppliedInput);
  const input = {
    concern: suppliedInput.concern.trim(),
    answers: Object.fromEntries(Object.entries(suppliedInput.answers ?? {}).map(([id,a]) => [id, {status: a.status, value: a.status === 'answered' ? a.value.trim() : ''}])),
    excludedEvidenceIds: [...new Set(suppliedInput.excludedEvidenceIds ?? [])].sort(),
    claimEdits: Object.fromEntries(Object.entries(suppliedInput.claimEdits ?? {}).map(([id,value]) => [id,value.trim()])),
    revision: suppliedInput.revision ?? 1
  };
  const evidence = await Promise.all(fixture.evidence.map(async item => ({...clone(item), contentHash: await hash(item)})));
  const framework = {...clone(fixture.framework), status: 'draft', clinicalApproval: 'none', mode: 'synthetic_preview'};
  const fixtureHash = await hash(fixture);
  const inputHash = await hash(input);
  const snapshotHash = await hash({fixtureHash, inputHash});
  const snapshot = {id: `snapshot-${snapshotHash}`, fixtureHash, inputHash, coverage: fixture.coverage, ...(fixture.vaultSnapshot ? {vault: clone(fixture.vaultSnapshot)} : {})};
  const decisions = evidence.map(item => decide(item, new Set(input.excludedEvidenceIds)));
  const questions = buildQuestions(input, evidence, decisions);
  const brief = buildBrief(input, evidence, decisions, questions, snapshot, framework);
  const citations = [...new Map([...decisions, ...questions].filter(x => x.citation).map(x => [x.citation.label, x.citation])).values()];
  return {
    synthetic: true, framework, snapshot, evidence, decisions, questions, brief,
    trace: {frameworkVersion: framework.version, snapshotId: snapshot.id, fixtureHash, inputHash, mode: 'synthetic_preview', clinicalApproval: 'none', citations, answers: clone(input.answers), concern: input.concern, decisions: clone(decisions), claimEdits: clone(input.claimEdits), inactiveBranches: [{ruleId: 'FAT-R004', reason: 'Mental-health screening requires reviewed handling; not implemented.'}, {ruleId: 'FAT-R101–104', reason: 'Post-COVID applicability and source update review are unresolved; not implemented.'}], ...(fixture.vaultSnapshot ? {vaultSnapshot: clone(fixture.vaultSnapshot), recordReviews: evidence.map(e => ({evidenceId: e.id, recordHash: e.recordHash, status: e.reviewStatus, basis: e.reviewStatus === 'reviewed' ? 'person_reviewed_in_session' : 'not_reviewed'}))} : {}), workflowVersion: 'fatigue-demo-2', model: null}
  };
}
