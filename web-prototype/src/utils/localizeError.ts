import { i18n } from '@/locales';
import type { ApiError } from '@/api/client';

/**
 * Resolve a user-facing message for an API error.
 * Strategy: look up `errors.<code>` in the active locale; fall back to the
 * server's `message` field; final fallback is a generic "request failed".
 *
 * The backend sends `{code, message, details}` but the frontend localizes
 * by `code` per VAL-I18N-CONTRACT-001.
 */
export function localizeApiError(err: ApiError): string {
  const t = i18n.global.t.bind(i18n.global);
  if (err.code) {
    const key = `errors.${err.code}`;
    const localized = t(key);
    // vue-i18n returns the key itself when no translation exists
    if (localized !== key) return localized;
  }
  if (err.message) return err.message;
  if (err.status >= 500) return t('errors.SERVER_ERROR');
  return t('errors.REQUEST_FAILED');
}

/** Regex to match the password segment in a DB connection URL. */
const CONN_STRING_PW_RE = /(:\/\/[^:]+:)([^@]+)(@)/g;

/**
 * Mask password segments in a database connection string.
 * Replaces the password portion between : and @ with ***.
 */
export function maskConnectionStringPw(url: string): string {
  return url.replace(CONN_STRING_PW_RE, '$1***$3');
}
