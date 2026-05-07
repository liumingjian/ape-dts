import { describe, expect, it } from 'vitest';
import dayjs from 'dayjs';

/** Shared logic — mirrors LicenseBanner / Dashboard.vue license-check logic. */
const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;

function shouldShowLicenseBanner(license: {
  status?: string;
  expireAt?: string;
} | null): boolean {
  if (!license) return false;
  if (license.status === 'expired' || license.status === 'expiring_soon') return true;
  if (!license.expireAt) return false;
  const diff = new Date(license.expireAt).getTime() - Date.now();
  return diff > 0 && diff <= THIRTY_DAYS_MS;
}

describe('Dashboard License Banner logic', () => {
  it('shows banner when status is expiring_soon', () => {
    expect(shouldShowLicenseBanner({ status: 'expiring_soon', expireAt: dayjs().add(7, 'day').toISOString() })).toBe(true);
  });

  it('shows banner when status is expired', () => {
    expect(shouldShowLicenseBanner({ status: 'expired', expireAt: dayjs().subtract(1, 'day').toISOString() })).toBe(true);
  });

  it('shows banner when expiry ≤30 days away even with status=active', () => {
    expect(shouldShowLicenseBanner({ status: 'active', expireAt: dayjs().add(20, 'day').toISOString() })).toBe(true);
  });

  it('suppresses banner when expiry >30 days away', () => {
    expect(shouldShowLicenseBanner({ status: 'active', expireAt: dayjs().add(60, 'day').toISOString() })).toBe(false);
  });

  it('suppresses banner when license is null', () => {
    expect(shouldShowLicenseBanner(null)).toBe(false);
  });

  it('suppresses banner when status is missing and no expireAt', () => {
    expect(shouldShowLicenseBanner({ status: 'active' })).toBe(false);
  });

  it('shows banner at exactly 30 days boundary', () => {
    const expire = dayjs().add(30, 'day').toISOString();
    expect(shouldShowLicenseBanner({ status: 'active', expireAt: expire })).toBe(true);
  });
});
