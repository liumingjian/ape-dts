import { describe, expect, it } from 'vitest';
import { readFileSync } from 'fs';
import { resolve } from 'path';

const ROOT = resolve(__dirname, '../../src');

/**
 * Bug 1: LicenseBanner renders TWICE — MainLayout + Dashboard.vue.
 * Fix: Remove from Dashboard.vue so only MainLayout has it.
 * This test verifies the component is used exactly once in the layout tree.
 */
describe('LicenseBanner single-instance invariant', () => {
  it('MainLayout template contains LicenseBanner', () => {
    const source = readFileSync(resolve(ROOT, 'layouts/MainLayout.vue'), 'utf-8');
    expect(source).toContain('LicenseBanner');
  });

  it('Dashboard.vue does NOT contain LicenseBanner (fix removes duplicate)', () => {
    const source = readFileSync(resolve(ROOT, 'views/dashboard/Dashboard.vue'), 'utf-8');
    // After fix, Dashboard.vue should NOT import or render LicenseBanner
    expect(source).not.toContain('LicenseBanner');
  });
});
