import { describe, expect, it } from 'vitest';
import zhCN from '@/locales/zh-CN.json';
import enUS from '@/locales/en-US.json';

type LocaleNode = string | { [k: string]: LocaleNode };

function flatten(node: LocaleNode, prefix = ''): Record<string, string> {
  if (typeof node === 'string') return { [prefix]: node };
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(node)) {
    const path = prefix ? `${prefix}.${k}` : k;
    Object.assign(out, flatten(v as LocaleNode, path));
  }
  return out;
}

describe('locale parity · zh-CN ↔ en-US', () => {
  const zh = flatten(zhCN as LocaleNode);
  const en = flatten(enUS as LocaleNode);
  const zhKeys = Object.keys(zh);
  const enKeys = Object.keys(en);

  it('en-US is a superset of zh-CN keys', () => {
    const missing = zhKeys.filter((k) => !(k in en));
    expect(missing, `en-US missing ${missing.length} keys: ${missing.slice(0, 10).join(', ')}`)
      .toEqual([]);
  });

  it('en-US has no orphan keys absent from zh-CN', () => {
    const extra = enKeys.filter((k) => !(k in zh));
    expect(extra, `en-US has ${extra.length} extra keys: ${extra.slice(0, 10).join(', ')}`)
      .toEqual([]);
  });

  it('every value is a non-empty string on both sides', () => {
    const blank = (m: Record<string, string>, label: string) =>
      Object.entries(m)
        .filter(([, v]) => typeof v !== 'string' || v.trim() === '')
        .map(([k]) => `${label}:${k}`);
    const offenders = [...blank(zh, 'zh'), ...blank(en, 'en')];
    expect(offenders).toEqual([]);
  });

  it('errors namespace has identical key sets', () => {
    const zhErrors = Object.fromEntries(Object.entries(zh).filter(([k]) => k.startsWith('errors.')));
    const enErrors = Object.fromEntries(Object.entries(en).filter(([k]) => k.startsWith('errors.')));
    const zhErrKeys = Object.keys(zhErrors);
    const enErrKeys = Object.keys(enErrors);
    expect(zhErrKeys.sort()).toEqual(enErrKeys.sort());
  });
});
