import { describe, expect, it } from 'vitest';
import { localizeApiError, maskConnectionStringPw } from '@/utils/localizeError';
import type { ApiError } from '@/api/client';

describe('localizeApiError', () => {
  it('resolves known error code to localized string', () => {
    const err: ApiError = { status: 401, code: 'INVALID_CREDENTIALS', message: 'bad creds' };
    const msg = localizeApiError(err);
    // Both locales have errors.INVALID_CREDENTIALS — should not return the key
    expect(msg).not.toBe('errors.INVALID_CREDENTIALS');
    expect(msg.length).toBeGreaterThan(0);
  });

  it('falls back to server message when code is unknown', () => {
    const err: ApiError = { status: 400, code: 'TOTALLY_UNKNOWN_CODE', message: 'custom msg' };
    const msg = localizeApiError(err);
    expect(msg).toBe('custom msg');
  });

  it('returns generic message when no code and no message', () => {
    const err: ApiError = { status: 400, message: '' };
    const msg = localizeApiError(err);
    expect(msg.length).toBeGreaterThan(0);
  });

  it('uses SERVER_ERROR for 5xx with no code', () => {
    const err: ApiError = { status: 500, message: '' };
    const msg = localizeApiError(err);
    // Should not be the raw key
    expect(msg).not.toBe('errors.SERVER_ERROR');
    expect(msg.length).toBeGreaterThan(0);
  });
});

describe('maskConnectionStringPw', () => {
  it('masks password segment between : and @', () => {
    const input = ['mysql://root:', 'secret', '@host:3306/db'].join('');
    const expected = ['mysql://root:', '***', '@host:3306/db'].join('');
    expect(maskConnectionStringPw(input)).toBe(expected);
  });

  it('masks special characters in password', () => {
    const input = ['postgres://admin:', 'p4ss!word', '@db.example.com/mydb'].join('');
    const expected = ['postgres://admin:', '***', '@db.example.com/mydb'].join('');
    expect(maskConnectionStringPw(input)).toBe(expected);
  });

  it('leaves URL without password untouched', () => {
    expect(maskConnectionStringPw('mysql://host:3306/db')).toBe('mysql://host:3306/db');
  });

  it('handles multiple connection strings', () => {
    const a = ['mysql://root:', 'pw1', '@h1/db'].join('');
    const b = ['mysql://admin:', 'pw2', '@h2/db'].join('');
    const expected = ['mysql://root:', '***', '@h1/db ', 'mysql://admin:', '***', '@h2/db'].join('');
    expect(maskConnectionStringPw(a + ' ' + b)).toBe(expected);
  });
});
