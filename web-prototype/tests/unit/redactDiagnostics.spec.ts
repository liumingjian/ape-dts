import { describe, expect, it } from 'vitest';
import { redactDiagnosticText, redactDiagnosticValue } from '@/utils/redactDiagnostics';

describe('diagnostic redaction', () => {
  it('redacts URL credentials and common secret assignments', () => {
    const result = redactDiagnosticText(
      'mysql://root:supersecret@db password=hunter2 token=abc Authorization: Bearer xyz',
    );
    expect(result).toBe('mysql://root:***@db password=*** token=*** Authorization: Bearer ***');
  });

  it('redacts nested values before diagnostics are copied', () => {
    const result = redactDiagnosticValue({
      message: 'password=hunter2',
      details: { endpoint: 'mysql://root:secret@db' },
    });
    expect(result).toEqual({
      message: 'password=***',
      details: { endpoint: 'mysql://root:***@db' },
    });
  });
});
