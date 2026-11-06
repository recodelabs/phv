import { evaluateCase } from './engine.mjs';
import { evaluatePersonalCase, DEMO_CONCERN } from './personal-engine.mjs';

const app = document.querySelector('#app');
const dialog = document.querySelector('#detail-dialog');
const dialogContent = document.querySelector('#dialog-content');
const printView = document.querySelector('#print-view');
const state = { fixture: null, result: null, mode: 'start', questionId: null, concern: '', answers: {}, answerDrafts: {}, excludedEvidenceIds: [], claimEdits: {}, revision: 0, appendix: false, busy: false, error: '', notice: '', vaultEnabled: false, personalEnabled: false, reviewedEvidenceIds: [] };

function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null || (value === false && key !== 'checked' && key !== 'disabled')) continue;
    if (key.startsWith('on')) node.addEventListener(key.slice(2).toLowerCase(), value);
    else if (key === 'class') node.className = value;
    else if (key === 'text') node.textContent = value;
    else if (key === 'checked') node.checked = value;
    else if (key === 'value') node.value = value;
    else if (key === 'disabled') node.disabled = value;
    else node.setAttribute(key, value);
  }
  for (const child of children.flat(Infinity)) if (child !== null && child !== undefined) node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  return node;
}
function button(label, handler, kind = '', props = {}) { return el('button', { type: 'button', class: `button ${kind}`, onClick: handler, ...props }, label); }
function linkButton(label, handler, props = {}) { return button(label, handler, 'link-button', props); }
function human(value) { return String(value ?? '').replaceAll('_', ' ').replaceAll('-', ' '); }
function asText(value) { return typeof value === 'string' ? value : JSON.stringify(value, null, 2); }
function citationNode(citation) {
  if (!citation) return el('p', { class: 'source-basis' }, 'No guideline citation is claimed for this decision.');
  const url = String(citation.url ?? '');
  const node = /^https?:\/\//i.test(url) ? el('a', { href: url, target: '_blank', rel: 'noopener noreferrer' }, citation.label ?? url) : el('span', {}, citation.label ?? 'Source locator unavailable');
  return el('div', { class: 'source-block' }, el('p', { class: 'eyebrow' }, 'Clinical source'), node, el('p', { class: 'source-basis' }, `${human(citation.support || 'adapted')} from guidance · local framework is a draft, not clinically approved.`));
}
function openDialog(title, ...children) {
  dialogContent.replaceChildren(el('div', { class: 'dialog-header' }, el('h2', { id: 'detail-title' }, title), button('Close', () => dialog.close(), 'small', { 'aria-label': 'Close details' })), ...children.filter(child => child !== null && child !== undefined));
  if (!dialog.open) dialog.showModal();
}
function personal() { return state.personalEnabled; }
function recordMode() { return state.vaultEnabled || personal(); }
function originalSources(item) {
  return (item.source.documents || []).map(doc => el('div', {class: 'source-block'},
    el('p', {}, doc.title || 'Original source'),
    doc.url ? el('a', {href: doc.url, target: '_blank', rel: 'noopener noreferrer', class: 'button small'}, 'Open original PDF') : null,
    el('p', {class: 'subtle'}, doc.locator || ''),
    el('p', {class: 'trace-id'}, `SHA-256 ${doc.sha256}`),
    doc.text ? el('pre', {class: 'source-quote'}, doc.text) : null));
}
function decisionFor(id) { return state.result?.decisions.find(item => item.evidenceId === id); }
function showEvidence(id) {
  const evidence = state.result.evidence.find(item => item.id === id);
  if (!evidence) return openDialog('Source unavailable', el('p', {}, 'This evidence reference could not be resolved.'));
  const decision = decisionFor(id);
  openDialog('Evidence and selection reason',
    el('div', { class: 'tag-list' }, el('span', { class: 'tag neutral' }, personal() ? 'Real historical lab result' : 'Fictional source'), el('span', { class: 'tag warning' }, human(decision?.disposition || 'unresolved'))),
    el('h3', {}, evidence.title),
    el('p', { class: 'subtle' }, `${evidence.source?.title || evidence.title} · ${asText(evidence.source?.locator || 'Fixture excerpt')} · ${asText(evidence.date || 'Date not recorded')}`),
    el('div', { class: 'source-quote' }, evidence.text),
    ...originalSources(evidence),
    el('hr', { class: 'divider' }),
    el('h3', {}, 'Why this decision?'), el('p', {}, decision?.rationale || 'No selection reason is available.'),
    el('p', { class: 'source-basis' }, `Basis: ${human(decision?.basis || 'unresolved')} ${decision?.ruleId ? `· Rule ${decision.ruleId}` : ''}`),
    citationNode(decision?.citation),
    el('p', { class: 'trace-id' }, `Evidence ${evidence.id} · Decision ${decision?.id || 'unavailable'} · Snapshot ${state.result.snapshot.id}`));
}
function showQuestionReason(question) {
  openDialog('Why ask?', el('h3', {}, question.text), el('p', {}, question.rationale), citationNode(question.citation),
    ...(question.evidenceIds || []).map(id => button(`Inspect ${state.result.evidence.find(item => item.id === id)?.title || id}`, () => showEvidence(id), 'small')));
}
function showClaim(claim) {
  const blocks = [el('p', {}, claim.text), el('p', { class: 'source-basis' }, `Attribution: ${human(claim.attribution)}`)];
  for (const id of claim.evidenceIds || []) {
    const item = state.result.evidence.find(item => item.id === id);
    blocks.push(el('div', { class: 'source-block' }, el('h3', {}, item?.title || id), el('p', { class: 'subtle' }, `${item?.source?.title || ''} · ${asText(item?.source?.locator || '')}`), el('p', { class: 'source-quote' }, item?.text || 'Source is unavailable.'), ...originalSources(item), button('Why included?', () => showEvidence(id), 'small')));
  }
  for (const id of claim.answerIds || []) {
    const question = state.result.questions.find(q => q.id === id);
    const answer = id === 'concern' ? { status: 'answered', value: state.concern } : state.answers[id];
    blocks.push(el('div', { class: 'source-block' }, el('h3', {}, question?.text || human(id)), el('p', {}, answer?.value || human(answer?.status || 'not answered')), el('p', { class: 'source-basis' }, personal() ? 'Hypothetical answer for this demo only. Not part of the health record.' : 'Attributed to the person’s answer in this session.')));
  }
  if (state.claimEdits[claim.id]) blocks.push(el('p', { class: 'scenario-box' }, personal() ? 'This is a hypothetical wording correction. The original finding is retained separately.' : 'This wording was edited by the person. The original source remains available above.'));
  blocks.push(el('p', { class: 'trace-id' }, `Claim ${claim.id} · Decisions ${(claim.decisionIds || []).join(', ') || 'none'} · Revision ${state.result.brief.revision}`));
  openDialog('Trace this statement', ...blocks);
}

async function evaluate(changes = {}) {
  if (state.busy) return false;
  const previous = { ...state, answers: { ...state.answers }, excludedEvidenceIds: [...state.excludedEvidenceIds], claimEdits: { ...state.claimEdits } };
  Object.assign(state, changes);
  state.busy = true; state.error = ''; state.notice = '';
  try {
    if (state.fixture.personalSnapshot) {
      const response = await fetch('./personal-case', {cache: 'no-store'});
      if (!response.ok || (await response.json()).personalSnapshot.id !== state.fixture.personalSnapshot.id) throw new Error('The real reports changed or are unavailable. Reopen and review the reports before generating another brief. Your previous draft is retained.');
    }
    if (state.fixture.vaultSnapshot) {
      const response = await fetch('./vault-snapshot', {cache: 'no-store'});
      if (!response.ok) throw new Error('The vault could not be checked. Retry, or re-import and review the records.');
      const current = await response.json();
      if (current.vaultSnapshot.id !== state.fixture.vaultSnapshot.id) throw new Error('The vault changed after import. Re-import and review its records before generating another brief. Your previous draft is still available.');
    }
    const revision = state.revision + 1;
    const fixture = recordMode() ? {...state.fixture, evidence: state.fixture.evidence.map(item => ({...item, reviewStatus: state.reviewedEvidenceIds.includes(item.id) ? 'reviewed' : 'unreviewed'}))} : state.fixture;
    const result = await (personal() ? evaluatePersonalCase : evaluateCase)(fixture, { concern: state.concern, answers: state.answers, excludedEvidenceIds: state.excludedEvidenceIds, claimEdits: state.claimEdits, revision });
    state.result = result; state.revision = revision;
    return true;
  } catch (error) {
    Object.assign(state, previous); state.error = error.message || 'We could not update this draft. Your last draft remains available.';
    return false;
  } finally { state.busy = false; }
}
function steps() {
  return el('ol', { class: 'steps', 'aria-label': 'Preparation steps' }, ...[...(recordMode() ? [['records', personal() ? 'Check real reports' : 'Import and check records']] : []), ['start', 'Your concern'], ['questions', 'Essential questions'], ['review', 'Review and export']].map(([mode, label], index) => el('li', { class: `step ${state.mode === mode ? 'active' : ''}`, ...(state.mode === mode ? { 'aria-current': 'step' } : {}) }, el('span', { class: 'step-number' }, index + 1), label)));
}
function intro() {
  return el('div', {}, el('p', { class: 'eyebrow' }, 'A focused brief for your doctor'), el('h1', {}, ['import', 'records', 'start'].includes(state.mode) ? 'Bring the right history.' : state.mode === 'questions' ? 'A few useful details.' : 'Your story, with its sources.'), el('p', { class: 'lead' }, ['import', 'records', 'start'].includes(state.mode) ? (personal() ? 'Use real historical lab results and a hypothetical concern to prepare a source-linked appointment brief.' : 'Explore how fictional health records and a short conversation become a clear summary about fatigue.') : state.mode === 'questions' ? 'Answer what you can. You can review a partial brief at any time.' : 'Check the wording, follow a source, and take a copy to your appointment.'), steps());
}
function render() {
  const alert = state.error ? el('div', { role: 'alert', class: 'error' }, state.error) : null;
  const content = state.mode === 'import' ? renderImport() : state.mode === 'records' ? renderRecords() : state.mode === 'start' ? renderStart() : state.mode === 'questions' ? renderQuestion() : renderReview();
  app.replaceChildren(intro(), ...(alert ? [alert] : []), el('div', { class: 'workspace' }, content, renderSidebar()));
  updatePrint();
}
async function importVault() {
  state.error = ''; state.busy = true; render();
  try {
    const response = await fetch(personal() ? './personal-case' : './vault-snapshot', {cache: 'no-store'});
    if (!response.ok) throw new Error(personal() ? 'The real reports could not be opened. Check the private manifest and source PDFs, then retry.' : 'The synthetic vault could not be imported. Check the vault and retry.');
    const fixture = await response.json();
    reset(false);
    state.fixture = fixture; state.mode = 'records'; state.reviewedEvidenceIds = [];
  } catch (error) { state.error = error.message; }
  finally { state.busy = false; render(); }
}
function renderImport() {
  return el('section', {class: 'card'}, el('h2', {}, personal() ? 'Your real reports, a hypothetical concern' : 'Open the synthetic vault'),
    el('p', {}, personal() ? 'Review a selected set of results beside the original PDFs. Hypothetical symptoms and answers stay in this page and are never written to your health record.' : 'Import a read-only copy of the configured fictional records, then check the evidence you want to use.'),
    button(personal() ? 'Open real lab reports' : 'Import vault snapshot', importVault, 'primary', {disabled: state.busy}));
}
function renderRecords() {
  const fixture = state.fixture, reviewed = state.reviewedEvidenceIds;
  return el('section', {class: 'card', 'aria-label': 'Review imported records'},
    el('h2', {}, 'Check the imported records'), el('p', {role: 'status'}, `${fixture.evidence.length} records imported · ${reviewed.length} reviewed`),
    el('p', {class: 'help'}, 'Check each structured record and its source. Only evidence you mark reviewed can enter the brief. Your choices apply to this snapshot and stay in this page.'),
    ...fixture.evidence.map(item => el('article', {class: 'evidence-item'}, el('h3', {}, item.title), el('p', {}, item.text), el('p', {class: 'subtle'}, `${item.date}${item.dateBasis ? ` · ${item.dateBasis}` : ''}`),
      button('Inspect imported source', () => openDialog(item.title,
        el('p', {class: 'source-quote'}, item.text), el('p', {class: 'trace-id'}, item.source.locator),
        personal() ? el('dl', {class: 'printed-fields'}, ...[['Printed test',item.printedName],['Printed value',item.printedValue],['Printed unit',item.printedUnit],['Printed reference',item.referenceRange],['Date basis',item.dateBasis]].flatMap(([label,value]) => [el('dt',{},label),el('dd',{},value)])) : null,
        ...originalSources(item),
        item.source.documents.length ? null : el('p', {}, 'No original-document provenance is available. Review here covers the structured record only.')), 'small'),
      el('label', {class: 'checkbox-label'}, el('input', {type: 'checkbox', checked: reviewed.includes(item.id), 'aria-label': `Reviewed: ${item.title}`, onChange: event => {
        state.reviewedEvidenceIds = event.target.checked ? [...state.reviewedEvidenceIds, item.id] : state.reviewedEvidenceIds.filter(id => id !== item.id);
        delete state.claimEdits[`record-${item.id}`]; state.result = null; render();
      }}), 'I checked this record'))),
    fixture.vaultSnapshot?.omitted.length ? el('details', {}, el('summary', {}, 'Records outside this preview'), ...fixture.vaultSnapshot.omitted.map(item => el('p', {}, `${item.resourceRef}: ${item.reason}`))) : null,
    el('div', {class: 'actions'}, button('Continue to concern →', () => { state.mode = 'start'; render(); }, 'primary'), button(personal() ? 'Reopen reports / clear session' : 'Re-import vault', importVault)),
    el('p', {class: 'help'}, 'Continuing with unchecked records keeps them outside the brief. No original record is changed.'));
}
function renderStart() {
  const textarea = el('textarea', { id: 'concern', name: 'concern', rows: '5', placeholder: 'Describe the tiredness you want to discuss…', value: state.concern, 'aria-describedby': 'concern-help', ...(state.error ? { 'aria-invalid': 'true' } : {}) });
  const form = el('form', { onSubmit: async event => { event.preventDefault(); if (await evaluate({ concern: textarea.value })) { state.mode = 'questions'; state.questionId = state.result.questions.find(q => q.answerStatus === 'unanswered')?.id || null; } render(); } },
    el('div', { class: 'tag-list' }, el('span', { class: 'tag' }, 'Fatigue'), el('span', { class: 'tag neutral' }, personal() ? 'Real labs · hypothetical concern' : 'Adult primary care · Portugal')),
    el('h2', {}, 'What would you like help preparing?'), el('label', { class: 'field-label', for: 'concern' }, 'Describe your concern'), textarea,
    el('p', { id: 'concern-help', class: 'help' }, personal() ? 'Describe a hypothetical concern. Every answer and correction stays separate from your real lab records and clears on refresh.' : 'Use your own words for the fictional person. Approximate dates are fine. Please do not enter real health information.'),
    personal() ? button('Use the hypothetical fatigue concern', () => { textarea.value = DEMO_CONCERN; }) : null,
    el('div', { class: 'actions' }, el('button', { type: 'submit', class: 'button primary', disabled: state.busy }, 'Find relevant evidence →')), recordMode() ? linkButton('← Review imported records', () => { state.mode = 'records'; render(); }) : null);
  return el('section', { class: 'card', 'aria-label': 'Describe your concern' }, form);
}
function renderQuestion() {
  const questions = state.result.questions;
  const question = questions.find(q => q.id === state.questionId) || questions.find(q => q.answerStatus === 'unanswered');
  if (!question) return el('section', { class: 'card' }, el('span', { class: 'tag' }, 'Ready to review'), el('h2', {}, 'You can finish with what we have.'), el('p', { class: 'subtle' }, 'Your brief retains any uncertainty and unanswered points.'), button('Review brief →', () => { state.mode = 'review'; render(); }, 'primary'));
  state.questionId = question.id;
  const index = questions.indexOf(question), answer = state.answers[question.id];
  const textarea = el('textarea', { id: 'question-answer', name: question.id, rows: '4', value: state.answerDrafts[question.id] ?? (answer?.status === 'answered' ? answer.value : ''), onInput: event => { state.answerDrafts[question.id] = event.target.value; }, placeholder: question.id === 'onset' ? 'For example, around the middle of September…' : 'Write your answer in your own words…' });
  async function save(status) {
    const answers = { ...state.answers, [question.id]: { status, value: status === 'answered' ? textarea.value : '' } };
    if (await evaluate({ answers })) {
      delete state.answerDrafts[question.id];
      state.questionId = state.result.questions.slice(index + 1).find(q => q.answerStatus === 'unanswered')?.id || state.result.questions.find(q => q.answerStatus === 'unanswered')?.id || null;
      if (!state.questionId) state.mode = 'review';
    }
    render();
  }
  return el('section', { class: 'card', 'aria-label': 'Essential questions' },
    el('div', { class: 'row between question-meta' }, el('span', { class: 'tag neutral' }, human(question.priority)), el('span', { class: 'question-count' }, `Question ${index + 1} of ${questions.length}`)),
    el('form', { onSubmit: event => { event.preventDefault(); save('answered'); } }, el('h2', {}, question.text), el('label', { class: 'field-label', for: 'question-answer' }, personal() ? 'Hypothetical answer' : 'Your answer'), textarea,
      el('p', { class: 'help' }, personal() ? 'Demo answer only. It cannot confirm a missing fact in the real report or change your health record.' : 'Uncertainty is useful information. Nothing here changes an original record.'),
      el('div', { class: 'row' }, linkButton('Why ask?', () => showQuestionReason(question)), answer ? el('span', { class: 'subtle' }, `Previously ${human(answer.status)}`) : null),
      el('div', { class: 'actions' }, el('button', { type: 'submit', class: 'button primary' }, index === questions.length - 1 ? 'Save and review →' : 'Save and continue →'), button('Not sure', () => save('unknown')), button('Skip', () => save('skipped')))),
    el('hr', { class: 'divider' }),
    el('div', { class: 'row between' }, index > 0 ? linkButton('← Previous question', () => { state.questionId = questions[index - 1].id; render(); }) : linkButton('← Edit concern', () => { state.mode = 'start'; render(); }), linkButton('Review with what we have', () => { state.mode = 'review'; render(); })),
    question.priority === 'optional' ? el('p', { class: 'help' }, 'You can finish now. This additional detail is optional.') : null);
}
function renderSidebar() {
  const fixture = state.fixture;
  const coverage = state.result?.snapshot?.coverage ?? fixture?.coverage;
  const context = el('section', { class: 'card' }, el('p', { class: 'eyebrow' }, 'The demonstration case'), el('h2', {}, fixture?.patient?.label ?? (personal() ? 'My lab history' : 'Example person')), el('div', { class: 'scenario-box' }, el('p', {}, personal() ? 'Real historical lab results paired with an invented fatigue concern for this demonstration.' : 'An adult in Portugal is preparing to discuss tiredness with a primary care doctor.'), el('p', {}, personal() ? 'The symptom story, sleep and medication answers are hypothetical. No current symptoms or medicines are established by these reports.' : 'The fictional records include a visit note, medicines and laboratory results.')),
    el('p', { class: 'coverage' }, coverage ? asText(coverage) : personal() ? 'Open the real lab reports to review selected results.' : 'Import the synthetic vault to see its records.'), recordMode() ? linkButton(personal() ? 'Reopen reports / clear session' : 'Re-import vault snapshot', importVault) : null,
    el('hr', { class: 'divider' }), el('p', { class: 'subtle' }, personal() ? 'This curated demo selects evidence for the stated scenario. It does not interpret free text, diagnose fatigue or assess urgency.' : 'Selection is demonstrative. The draft framework has not been clinically approved for use in Portugal.'),
    el('details', {}, el('summary', {}, 'Guidance and review status'), ...(state.result?.framework?.sources || fixture?.framework?.sources || []).map(source => el('div', { class: 'source-block' }, el('h3', {}, source.title), el('p', { class: 'subtle' }, `${source.version || 'Edition not recorded'} · ${source.jurisdiction || 'Jurisdiction not recorded'}`), el('p', { class: 'subtle' }, `Publisher status: ${human(source.status || 'unknown')}. Local approval: ${human(source.localApproval || 'pending')}.`)))));
  const sidebar = el('aside', { class: 'sidebar', 'aria-label': 'Records and preparation context' }, context);
  if (state.result) {
    sidebar.append(el('section', { class: 'card' }, el('h3', {}, personal() ? 'Hypothetical answers' : 'Your answers'), el('div', { class: 'question-nav' }, ...state.result.questions.map(q => el('button', { type: 'button', class: q.id === state.questionId && state.mode === 'questions' ? 'current' : '', onClick: () => { state.mode = 'questions'; state.questionId = q.id; render(); } }, el('span', {}, human(q.id)), el('span', {}, human(q.answerStatus)))))));
    const evidence = el('details', { class: 'card', open: state.mode === 'questions' ? '' : undefined }, el('summary', {}, `Evidence selection · ${state.result.evidence.length} records`), el('p', { class: 'section-note' }, 'Inspect a source and the reason it was selected, deferred or left out.'), el('div', { class: 'evidence-list' }, ...state.result.evidence.map(renderEvidence)));
    if (state.mode !== 'questions') evidence.removeAttribute('open');
    sidebar.append(evidence);
    sidebar.append(el('section', { class: 'card' }, el('p', { class: 'eyebrow' }, 'This session'), el('p', { class: 'subtle' }, `Brief revision ${state.result.brief.revision} · local only`), linkButton('Start over', () => reset())));
  }
  return sidebar;
}
function renderEvidence(item) {
  const decision = decisionFor(item.id), excluded = state.excludedEvidenceIds.includes(item.id);
  const disposition = decision?.disposition || 'unresolved';
  const eligible = ['include', 'included'].includes(disposition) || excluded;
  return el('article', { class: 'evidence-item' }, el('h3', {}, item.title), el('p', { class: 'subtle' }, asText(item.date || 'Date not recorded')), el('div', { class: 'tag-list' }, el('span', { class: `tag ${eligible && !excluded ? '' : 'neutral'}` }, human(disposition))),
    el('div', { class: 'row' }, linkButton('Source and reason', () => showEvidence(item.id)), eligible ? linkButton(excluded ? 'Restore to brief' : 'Remove from brief', () => toggleEvidence(item, excluded)) : null));
}
async function toggleEvidence(item, excluded) {
  async function apply() {
    const ids = excluded ? state.excludedEvidenceIds.filter(id => id !== item.id) : [...state.excludedEvidenceIds, item.id];
    const edits = { ...state.claimEdits };
    if (!excluded) delete edits[`record-${item.id}`];
    const success = await evaluate({ excludedEvidenceIds: ids, claimEdits: edits });
    if (success && dialog.open) dialog.close();
    render();
  }
  if (!excluded && state.claimEdits[`record-${item.id}`]) {
    openDialog('Remove this statement?', el('p', {}, 'You edited the statement based on this record. Removing it also removes your edited wording from this brief. The original record remains available.'), el('div', { class: 'actions' }, button('Remove statement and correction', apply, 'primary'), button('Keep statement', () => dialog.close())));
  } else await apply();
}
function editClaim(claim) {
  const field = el('textarea', { id: 'claim-edit', rows: '5', value: state.claimEdits[claim.id] ?? claim.text });
  const error = el('p', { role: 'alert', class: 'error', hidden: '' });
  const form = el('form', { onSubmit: async event => { event.preventDefault(); if (!field.value.trim()) { error.removeAttribute('hidden'); error.textContent = 'Enter the wording you want the brief to use.'; return; } const ok = await evaluate({ claimEdits: { ...state.claimEdits, [claim.id]: field.value } }); if (ok) dialog.close(); else { error.removeAttribute('hidden'); error.textContent = state.error; } render(); } },
    el('p', { class: 'subtle' }, personal() ? 'This adds a hypothetical wording correction alongside the original statement. It never updates your health record.' : 'Your correction will be attributed to you. The original source is kept for comparison.'), el('label', { class: 'field-label', for: 'claim-edit' }, 'Wording for the brief'), field, error,
    el('div', { class: 'actions' }, el('button', { type: 'submit', class: 'button primary' }, 'Save correction'), button('Cancel', () => dialog.close()), state.claimEdits[claim.id] ? button('Restore original wording', async () => { const edits = { ...state.claimEdits }; delete edits[claim.id]; if (await evaluate({ claimEdits: edits })) dialog.close(); render(); }) : null));
  openDialog('Correct a statement', form);
}
function renderReview() {
  const brief = state.result.brief;
  const card = el('section', { class: 'card', 'aria-label': 'Review your brief' },
    el('div', { class: 'brief-head' }, el('div', {}, el('p', { class: 'eyebrow' }, 'Consultation preparation'), el('h2', {}, 'Fatigue · doctor’s brief')), el('span', { class: 'tag neutral' }, `Revision ${brief.revision}`)),
    el('p', { class: 'subtle' }, personal() ? 'Real report findings and hypothetical answers are labelled separately below and in every export.' : 'Fictional example · based on the selected records and your answers.'));
  const groups = personal() ? [['hypothetical_answer', 'Hypothetical concern and answers'], ['source_fact', 'Real report findings'], ['hypothetical_correction', 'Hypothetical wording corrections']] : [['concern', 'Your concern'], ['timeline', 'Focused timeline'], ['history', 'Relevant history and results'], ['questions', 'Questions for the doctor']];
  for (const [key, title] of groups) {
    const claims = brief.claims.filter(c => personal() ? c.attribution === key : c.section === key);
    if (!claims.length) continue;
    card.append(el('section', { class: 'brief-section' }, el('h3', {}, title), ...claims.map(claim => el('article', { class: 'claim' }, el('p', {}, claim.text), el('div', { class: 'claim-meta' }, human(claim.attribution)), el('div', { class: 'row' }, linkButton('Sources and why included', () => showClaim(claim)), claim.attribution === 'hypothetical_correction' ? null : linkButton(personal() ? 'Add hypothetical correction' : 'Edit wording', () => editClaim(claim)))))));
  }
  if (brief.gaps.length) card.append(el('section', { class: 'brief-section' }, el('h3', {}, 'Gaps to keep visible'), el('ul', { class: 'gap-list' }, ...brief.gaps.map(gap => el('li', {}, typeof gap === 'string' ? gap : gap.text || gap.description || asText(gap))))));
  if (personal()) card.append(el('section', {class: 'brief-section'}, el('h3', {}, 'Questions for the hypothetical appointment'), el('p', {}, 'How should the historical glucose and HbA1c results be interpreted together?'), el('p', {}, 'Given the new concern and age of the reports, is updated assessment appropriate?')));
  const checkbox = el('input', { id: 'appendix', type: 'checkbox', checked: state.appendix, onChange: event => { state.appendix = event.target.checked; updatePrint(); } });
  card.append(el('section', { class: 'export-panel', 'aria-label': 'Export brief' }, el('h3', {}, 'Take a copy'), el('label', { class: 'checkbox-label', for: 'appendix' }, checkbox, 'Include evidence appendix'), el('div', { class: 'actions' }, button('Copy brief', copyBrief, 'primary'), button('Download text', () => download('fatigue-brief.txt', exportText(), 'text/plain')), button('Print / Save PDF', () => { updatePrint(); window.print(); })), el('div', { class: 'actions' }, linkButton('Download trace JSON', () => download('fatigue-trace.json', JSON.stringify({ synthetic: !personal(), mode: personal() ? 'personal_lab_demo' : 'synthetic_preview', inputKind: personal() ? 'hypothetical_session_only' : 'synthetic', input: { concern: state.concern, answers: state.answers, excludedEvidenceIds: state.excludedEvidenceIds, claimEdits: state.claimEdits }, result: state.result }, null, 2), 'application/json'))), el('p', { class: 'help' }, 'Both exports use this revision. Save as PDF through your browser’s print dialog.'), state.notice ? el('p', { role: 'status', class: 'status' }, state.notice) : null));
  card.append(el('div', { class: 'actions' }, linkButton('← Review or change answers', () => { state.mode = 'questions'; state.questionId = state.result.questions[0]?.id; render(); })));
  return card;
}
function appendixText() {
  const lines = ['EVIDENCE APPENDIX', 'Selection framework: draft, not clinically approved.'];
  const ids = new Set(state.result.brief.claims.flatMap(claim => claim.evidenceIds || []));
  for (const item of state.result.evidence.filter(item => ids.has(item.id))) {
    const decision = decisionFor(item.id);
    lines.push('', `[${item.id}] ${item.title}`, `${item.source?.title || item.title} · ${asText(item.source?.locator || '')} · ${asText(item.date || '')}`, item.text, `Selection reason: ${decision?.rationale || 'Unavailable'}`, `Basis: ${human(decision?.basis || 'unresolved')}`);
    if (decision?.citation) lines.push(`Adapted guidance: ${decision.citation.label}`, decision.citation.url);
  }
  return lines.join('\n');
}
function exportText() { return state.result ? state.result.brief.text + (state.appendix ? `\n\n${appendixText()}` : '') : ''; }
function updatePrint() { printView.textContent = exportText(); }
function download(name, content, type) {
  const url = URL.createObjectURL(new Blob([content], { type: `${type};charset=utf-8` }));
  const anchor = el('a', { href: url, download: name }); document.body.append(anchor); anchor.click(); anchor.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  state.notice = `${name} prepared from revision ${state.result.brief.revision}.`; render();
}
async function copyBrief() {
  try { await navigator.clipboard.writeText(exportText()); state.notice = `Brief revision ${state.result.brief.revision} copied.`; render(); }
  catch { const field = el('textarea', { class: 'copy-text', readonly: '', 'aria-label': 'Brief text to copy', value: exportText() }); openDialog('Copy your brief', el('p', { class: 'subtle' }, 'Clipboard access is unavailable. Select and copy the text below.'), field); field.select(); }
}
function reset(shouldRender = true) { Object.assign(state, { result: null, mode: 'start', questionId: null, concern: '', answers: {}, answerDrafts: {}, excludedEvidenceIds: [], claimEdits: {}, revision: 0, appendix: false, busy: false, error: '', notice: '', reviewedEvidenceIds: [] }); if (recordMode()) state.mode = 'import'; if (shouldRender) render(); }

async function load() {
  try {
    const config = await fetch('./preview-config', {cache: 'no-store'});
    if (!config.ok) throw new Error('The preview configuration could not be opened.');
    const settings = await config.json();
    state.vaultEnabled = settings.vaultEnabled; state.personalEnabled = settings.personalEnabled;
    if (personal()) {
      document.querySelector('.demo-notice').replaceChildren(el('strong', {}, 'Real labs · hypothetical concern'), el('span', {}, 'Local demo · answers never enter your health record · no urgency assessment.'));
      document.querySelector('.site-footer').replaceChildren(el('span', {}, 'Original lab reports are read-only. No information is sent to a doctor.'), el('span', {}, 'Hypothetical answers stay in this page. Refreshing clears them.'));
    }
    if (recordMode()) { state.mode = 'import'; state.fixture = null; render(); return; }
    const response = await fetch('./fixture.json', { cache: 'no-store' });
    if (!response.ok) throw new Error('The synthetic case could not be opened.');
    state.fixture = await response.json(); render();
  } catch (error) {
    app.replaceChildren(el('section', { class: 'card' }, el('h1', {}, 'The case could not be opened.'), el('p', { role: 'alert', class: 'error' }, error.message), button('Try again', load, 'primary')));
  }
}
load();
