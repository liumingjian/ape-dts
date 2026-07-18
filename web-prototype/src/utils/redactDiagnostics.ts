const URL_CREDENTIALS = /([a-z][a-z0-9+.-]*:\/\/[^\s/:@]+:)[^\s@]+(@)/gi;
const KEY_VALUE_SECRET = /\b(password|passwd|pwd|token|secret|access_key|secret_key)\s*=\s*([^\s,;]+)/gi;
const AUTHORIZATION = /(authorization\s*[:=]\s*(?:bearer\s+)?)([^\s,;]+)/gi;

export function redactDiagnosticText(value: string): string {
  return value
    .replace(URL_CREDENTIALS, '$1***$2')
    .replace(KEY_VALUE_SECRET, '$1=***')
    .replace(AUTHORIZATION, '$1***');
}

export function redactDiagnosticValue<T>(value: T): T {
  if (typeof value === 'string') return redactDiagnosticText(value) as T;
  if (Array.isArray(value)) return value.map(redactDiagnosticValue) as T;
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, nested]) => [key, redactDiagnosticValue(nested)]),
    ) as T;
  }
  return value;
}
