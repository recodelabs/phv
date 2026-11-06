import { readFile } from 'node:fs/promises';
import { evaluateCase } from './engine.mjs';

const args = process.argv.slice(2);
if (args.includes('--help')) {
  console.log('Usage: node cli.mjs [--input answers.json] [--trace]\nRuns only the bundled synthetic case. With no input, uses an illustrative completed conversation.');
} else {
  try {
    let inputPath;
    let trace = false;
    for (let i = 0; i < args.length; i++) {
      if (args[i] === '--trace') trace = true;
      else if (args[i] === '--input' && args[i + 1]) inputPath = args[++i];
      else throw new Error('Usage: node cli.mjs [--input answers.json] [--trace]');
    }
    const fixture = JSON.parse(await readFile(new URL('./fixture.json', import.meta.url), 'utf8'));
    const input = inputPath ? JSON.parse(await readFile(inputPath, 'utf8')) : {
      concern: 'I have been unusually tired and want to prepare for my primary care visit.',
      answers: {
        onset: { status: 'answered', value: 'Around mid September 2026; I am not sure of the exact date.' },
        impact: { status: 'answered', value: 'I need more breaks while doing ordinary household tasks.' },
        medication_current: { status: 'unknown', value: '' },
        goal: { status: 'answered', value: 'Review the symptom timeline and my existing results with the doctor.' },
      },
      revision: 1,
    };
    const result = await evaluateCase(fixture, input);
    console.log(trace ? JSON.stringify(result, null, 2) : result.brief.text);
  } catch (error) {
    console.error(`Cannot generate brief: ${error.message}`);
    process.exitCode = 1;
  }
}
