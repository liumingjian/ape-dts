#!/usr/bin/env node
// Flatten all "a.b.c" leaf keys in locale JSONs into a pure nested object tree.
// When a key collides with a sibling that is itself a string (e.g. "nav.tasks"
// is "任务管理" while "nav.tasks.cdc" wants nav.tasks.cdc), the original string
// is moved to a `_label` child so the parent tree stays accessible via
// t('nav.tasks._label'). Idempotent.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const localeDir = path.resolve(__dirname, '..', 'src', 'locales');

const LABEL_KEY = '_label';

function setPath(root, parts, value) {
  let cur = root;
  for (let i = 0; i < parts.length - 1; i++) {
    const seg = parts[i];
    const next = cur[seg];
    if (typeof next === 'string') {
      // Demote sibling string to _label.
      cur[seg] = { [LABEL_KEY]: next };
    } else if (next == null || typeof next !== 'object') {
      cur[seg] = {};
    }
    cur = cur[seg];
  }
  const leaf = parts[parts.length - 1];
  if (typeof cur[leaf] === 'object' && cur[leaf] !== null) {
    // Existing object; place value under _label.
    cur[leaf][LABEL_KEY] = value;
  } else {
    cur[leaf] = value;
  }
}

function normalize(node) {
  if (node === null || typeof node !== 'object' || Array.isArray(node)) return node;
  // First, normalize children depth-first so nested objects become pure trees.
  for (const k of Object.keys(node)) {
    node[k] = normalize(node[k]);
  }
  // Then, expand any flat dot-keys at this level.
  const flatKeys = Object.keys(node).filter((k) => k.includes('.'));
  if (flatKeys.length === 0) return node;
  const result = {};
  // Preserve non-dot keys first.
  for (const k of Object.keys(node)) {
    if (!k.includes('.')) result[k] = node[k];
  }
  for (const k of flatKeys) {
    setPath(result, k.split('.'), node[k]);
  }
  return result;
}

function go(file) {
  const p = path.join(localeDir, file);
  const data = JSON.parse(fs.readFileSync(p, 'utf8'));
  const out = normalize(data);
  fs.writeFileSync(p, JSON.stringify(out, null, 2) + '\n', 'utf8');
  console.log(`normalized ${file}`);
}

go('zh-CN.json');
go('en-US.json');
