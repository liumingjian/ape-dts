/**
 * Sanitise a post-login redirect path to prevent open-redirect attacks.
 * Only same-origin paths starting with a single `/` are allowed.
 * Returns `/dashboard` for any disallowed or empty input.
 */
export function sanitizeRedirect(raw: string | undefined): string {
  if (!raw) return '/dashboard';
  // Must start with / but not // (scheme-relative) or /\ (potential escape)
  if (!raw.startsWith('/') || raw.startsWith('//') || raw.startsWith('/\\')) return '/dashboard';
  return raw;
}
