import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const ROOT = resolve(__dirname, '../../src');

describe('TopBar mock SSE behaviour', () => {
  it('does not open the alert EventSource while MSW mock mode is active', () => {
    const source = readFileSync(resolve(ROOT, 'components/TopBar.vue'), 'utf-8');

    expect(source).toContain("const isMockMode = import.meta.env.VITE_USE_MOCK === 'true'");
    expect(source).toContain('auth.isAuthenticated && !isMockMode');
  });
});
