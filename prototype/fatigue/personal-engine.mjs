// This mode never treats hypothetical input as a health-record fact.
const definitions = [
  ['onset', 'When did the hypothetical tiredness start?', 'An approximate onset places historical tests before or during the scenario.', 'timeline', 'Hypothetical onset: '],
  ['impact', 'What has become harder in the hypothetical day?', 'Describe the effect on everyday life in this scenario.', 'concern', 'Hypothetical daily impact: '],
  ['fasting', 'Was the September glucose test taken fasting?', 'The report does not state fasting status. A demo answer cannot change the source document.', 'history', 'Hypothetical account of September fasting status: '],
  ['sleep', 'How has sleep been in this scenario?', 'Sleep is missing from these lab reports; this answer belongs only to the scenario.', 'history', 'Hypothetical sleep account: '],
  ['medication_current', 'What medicines or supplements are taken in this scenario?', 'These selected reports do not establish current medication use.', 'history', 'Hypothetical medication account: '],
  ['goal', 'What would you like to ask the doctor?', 'Choose the purpose of the hypothetical appointment.', 'questions', 'Hypothetical consultation goal: ']
];
export const DEMO_CONCERN = 'For about six weeks, I’ve felt more tired than usual and need more breaks during the day. I thought my previous blood tests were mostly normal, but I’m wondering whether blood sugar could be relevant. Help me prepare for a doctor’s appointment using the records I already have.';
const check = (ok, message) => { if (!ok) throw new Error(message); };
const hasText = value => typeof value === 'string' && value.trim().length > 0 && value.length <= 10000;
const clone = value => structuredClone(value);
async function hash(value) {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  return [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))].map(b => b.toString(16).padStart(2, '0')).join('');
}
export async function evaluatePersonalCase(fixture, input) {
  check(fixture?.mode === 'personal_lab_demo' && fixture.synthetic === false && fixture.personalSnapshot?.id, 'A configured real-lab demo snapshot is required.');
  check(hasText(input.concern), 'Describe the hypothetical concern before continuing (under 10,000 characters).');
  const answers = input.answers ?? {}, edits = input.claimEdits ?? {}, exclusions = input.excludedEvidenceIds ?? [];
  check(answers && typeof answers === 'object' && !Array.isArray(answers), 'Answers must be an object.');
  check(edits && typeof edits === 'object' && !Array.isArray(edits), 'Corrections must be an object.');
  check(Array.isArray(exclusions) && exclusions.every(id => fixture.evidence.some(e => e.id === id)), 'Unknown evidence exclusion.');
  check(Number.isSafeInteger(input.revision ?? 1) && (input.revision ?? 1) >= 1, 'Invalid brief revision.');
  for (const [id, answer] of Object.entries(answers)) {
    check(definitions.some(d => d[0] === id), 'Unknown hypothetical question.');
    check(answer && ['answered', 'unknown', 'skipped', 'declined'].includes(answer.status), 'Invalid hypothetical answer status.');
    check(answer.value === undefined || (typeof answer.value === 'string' && answer.value.length <= 10000), 'Answer must be text under 10,000 characters.');
    if (answer.status === 'answered') check(hasText(answer.value), 'Enter a hypothetical answer, or choose Not sure or Skip.');
  }
  const cleanAnswers = Object.fromEntries(Object.entries(answers).map(([id,a]) => [id, {status: a.status, value: a.status === 'answered' ? a.value.trim() : ''}]));
  const evidence = await Promise.all(fixture.evidence.map(async e => ({...clone(e), contentHash: await hash(e)})));
  const decisions = evidence.map(e => ({id: `decision-${e.id}`, evidenceId: e.id, ruleId: null, citation: null,
    disposition: exclusions.includes(e.id) ? 'exclude' : e.reviewStatus !== 'reviewed' ? 'defer-for-confirmation' : e.demoRole === 'outside_focus' ? 'exclude' : 'include',
    basis: exclusions.includes(e.id) ? 'user_request' : e.reviewStatus !== 'reviewed' ? 'record_review_policy' : 'curated_demo_selection',
    rationale: exclusions.includes(e.id) ? 'Removed from this brief only; the source report is unchanged.' : e.reviewStatus !== 'reviewed' ? 'Check this extraction against its PDF before including it.' : e.selectionReason}));
  const questions = definitions.map(([id,text,rationale]) => ({id,text,rationale, priority: id === 'goal' ? 'optional' : 'essential', evidenceIds: [], citation: null, answerStatus: cleanAnswers[id]?.status ?? 'unanswered'}));
  const claims = [{id: 'concern', section: 'concern', text: input.concern.trim(), attribution: 'hypothetical_answer', evidenceIds: [], decisionIds: [], answerIds: ['concern']}];
  for (const [id,,,section,prefix] of definitions) if (cleanAnswers[id]?.status === 'answered') claims.push({id, section, text: prefix + cleanAnswers[id].value, attribution: 'hypothetical_answer', evidenceIds: [], decisionIds: [], answerIds: [id]});
  for (const d of decisions.filter(d => d.disposition === 'include')) {
    const e = evidence.find(e => e.id === d.evidenceId);
    claims.push({id: `record-${e.id}`, section: 'history', text: `${e.date} (${e.dateBasis}): ${e.text}`, attribution: 'source_fact', evidenceIds: [e.id], decisionIds: [d.id], answerIds: []});
  }
  for (const [id,value] of Object.entries(edits)) {
    check(claims.some(c => c.id === id), 'This claim is unavailable. Restore its source before editing.');
    check(hasText(value), 'Enter the hypothetical wording you want the brief to use.');
  }
  // Preserve the source-derived claim and add the session correction separately.
  const corrected = claims.flatMap(c => Object.hasOwn(edits,c.id) ? [c, {...clone(c), id: `correction-${c.id}`, text: edits[c.id].trim(), originalText: c.text, attribution: 'hypothetical_correction', edited: true, correctionOfEvidenceIds: [...c.evidenceIds]}] : [c]);
  const statuses = {unanswered: 'Not answered', unknown: 'Not sure', skipped: 'Skipped', declined: 'Declined'};
  const gaps = questions.filter(q => q.answerStatus !== 'answered').map(q => `${q.text} — ${statuses[q.answerStatus]}.`);
  gaps.push('The source does not state whether September glucose was fasting. Hypothetical answers do not establish that fact.', 'These are historical reports; they do not establish current blood counts or glucose status.', 'The selected results do not establish a cause of fatigue. This demo does not assess urgency or generate a diagnosis, test order or treatment.');
  if (decisions.some(d => d.disposition === 'defer-for-confirmation')) gaps.push('Unchecked extractions remain outside the brief.');
  const revision = input.revision ?? 1;
  const fixtureHash = await hash(fixture), inputHash = await hash({concern: input.concern.trim(), answers: cleanAnswers, exclusions, edits});
  const snapshot = {id: `demo-${await hash({fixtureHash,inputHash})}`, fixtureHash, inputHash, coverage: fixture.coverage};
  const lines = ['REAL HISTORICAL LAB RESULTS + HYPOTHETICAL FATIGUE SCENARIO', 'Demonstration brief — not an actual patient symptom history', `Revision ${revision}`, `Coverage: ${fixture.coverage}`, '', 'Hypothetical concern and answers'];
  for (const c of corrected.filter(c => c.attribution === 'hypothetical_answer')) lines.push(`- ${c.text} [Hypothetical answer]`);
  lines.push('', 'Real report findings (reviewed extracts)');
  for (const c of corrected.filter(c => c.attribution === 'source_fact')) lines.push(`- ${c.text} ${c.evidenceIds.map(id => `[${id}]`).join(' ')}`);
  lines.push('', 'Hypothetical wording corrections (original findings retained)');
  for (const c of corrected.filter(c => c.attribution === 'hypothetical_correction')) lines.push(`- ${c.text} [Hypothetical correction to ${c.id.replace('correction-','')}]`);
  lines.push('', 'Questions for the hypothetical appointment', '- How should the historical glucose and HbA1c results be interpreted together?', '- Given the new concern and age of the reports, is updated assessment appropriate?', '', 'Gaps and uncertainty', ...gaps.map(g => `- ${g}`), '', 'Evidence index');
  for (const d of decisions.filter(d => d.disposition === 'include')) {
    const e = evidence.find(e => e.id === d.evidenceId);
    lines.push(`[${e.id}] ${e.source.title}; ${e.source.locator}; SHA-256 ${e.source.documents[0].sha256}`);
  }
  lines.push('', 'Hypothetical input is session-only and is never written to the health record.', `Snapshot: ${snapshot.id}`);
  return {mode: 'personal_lab_demo', synthetic: false, framework: {...clone(fixture.framework), status: 'draft', clinicalApproval: 'none'}, snapshot, evidence, decisions, questions,
    brief: {revision, claims: corrected, gaps, text: lines.join('\n')},
    trace: {mode: 'personal_lab_demo', snapshotId: snapshot.id, fixtureHash, inputHash, frameworkVersion: fixture.framework.version, citations: [],
      realRecordSnapshot: clone(fixture.personalSnapshot), hypotheticalSession: {concern: input.concern.trim(), answers: clone(cleanAnswers), claimEdits: clone(edits)},
      decisions: clone(decisions), recordReviews: evidence.map(e => ({evidenceId: e.id, contentHash: e.contentHash, status: e.reviewStatus})), clinicalApproval: 'none', model: null}};
}
