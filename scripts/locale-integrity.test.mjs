import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import ts from 'typescript';

const root = new URL('../', import.meta.url);
const locales = Object.fromEntries(['zh-CN', 'en-US'].map((name) => [name,
  JSON.parse(readFileSync(new URL(`src/locales/${name}.json`, root), 'utf8')),
]));
function flatten(value, prefix = '', output = new Map()) {
  for (const [key, item] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof item === 'string') output.set(path, item);
    else if (item && typeof item === 'object') flatten(item, path, output);
  }
  return output;
}
const dictionaries = Object.fromEntries(Object.entries(locales).map(([name, value]) => [name, flatten(value)]));

test('both languages expose the same translated keys', () => {
  assert.deepEqual([...dictionaries['en-US'].keys()].sort(), [...dictionaries['zh-CN'].keys()].sort());
});

test('translations retain their interpolation parameters', () => {
  const parameters = (text) => [...text.matchAll(/\{([A-Za-z_]\w*)\}/g)].map((match) => match[1]).sort();
  for (const [key, text] of dictionaries['zh-CN']) {
    assert.deepEqual(parameters(dictionaries['en-US'].get(key) ?? ''), parameters(text), key);
  }
});

test('literal UI translation calls resolve instead of displaying internal keys', () => {
  const failures = [];
  function visitDirectory(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visitDirectory(path);
      else if (/\.tsx?$/.test(entry.name)) {
        const source = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest, true);
        function visit(node) {
          if (ts.isCallExpression(node)
            && ((ts.isIdentifier(node.expression) && node.expression.text === 't')
              || (ts.isPropertyAccessExpression(node.expression) && node.expression.name.text === 't'))
            && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) {
            const key = node.arguments[0].text;
            for (const [locale, dictionary] of Object.entries(dictionaries)) {
              if (!dictionary.has(key)) failures.push(`${path}:${source.getLineAndCharacterOfPosition(node.pos).line + 1}: ${locale} ${key}`);
            }
          }
          ts.forEachChild(node, visit);
        }
        visit(source);
      }
    }
  }
  visitDirectory(fileURLToPath(new URL('src/', root)));
  assert.deepEqual(failures, []);
});
