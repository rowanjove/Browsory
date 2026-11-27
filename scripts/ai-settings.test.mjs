import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/services/saveAiSettings.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } });
const exports = {};
vm.runInNewContext(outputText, { exports, Error });
const { saveAiSettings, AiSettingsSaveError } = exports;
const fixture = { credentialRef: 'ai_credential_fixture', apiKey: 'synthetic-key', baseUrl: 'https://fixture.example/v1', model: 'fixture-model' };

test('a failed endpoint write reports the acknowledged credential write and stops', async () => {
  const stored = new Map();
  await assert.rejects(saveAiSettings(async (key, value) => {
    if (key === 'ai_base_url') throw new Error('synthetic database failure');
    stored.set(key, value);
  }, fixture), (error) => {
    assert.ok(error instanceof AiSettingsSaveError);
    assert.deepEqual(Array.from(error.confirmedFields), ['credential']);
    assert.equal(error.failedField, 'endpoint');
    assert.ok(!error.message.includes(fixture.apiKey));
    return true;
  });
  assert.deepEqual(Array.from(stored.keys()), ['ai_credential_fixture']);
});

test('model failure reports both completed writes rather than promising rollback', async () => {
  const written = [];
  await assert.rejects(saveAiSettings(async (key) => {
    if (key === 'ai_model') throw new Error('synthetic final write failure');
    written.push(key);
  }, fixture), (error) => {
    assert.deepEqual(Array.from(error.confirmedFields), ['credential', 'endpoint']);
    assert.equal(error.failedField, 'model');
    return true;
  });
  assert.deepEqual(written, ['ai_credential_fixture', 'ai_base_url']);
});

test('embedding saves the matching endpoint/model namespace and keeps the credential reference', async () => {
  const written = [];
  await saveAiSettings(async (key, value) => { written.push([key, value]); }, { ...fixture, embedding: true, credentialRef: 'ai_embedding_credential_fixture' });
  assert.deepEqual(written, [
    ['ai_embedding_credential_fixture', 'synthetic-key'],
    ['ai_embedding_base_url', fixture.baseUrl],
    ['ai_embedding_model', fixture.model],
  ]);
});
