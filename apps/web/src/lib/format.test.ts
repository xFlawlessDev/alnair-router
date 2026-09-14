import { describe, expect, it } from 'vitest';

import {
  formatCost,
  formatDateTime,
  formatLatency,
  isEnabled,
  maskSecret,
  parseHeaders,
  successRate,
} from './format';

describe('isEnabled', () => {
  it('treats SQLite integers as booleans', () => {
    expect(isEnabled(1)).toBe(true);
    expect(isEnabled(0)).toBe(false);
  });
});

describe('parseHeaders', () => {
  it('parses a JSON object and stringifies values', () => {
    expect(parseHeaders('{"X-Org":"acme","Retries":3}')).toEqual({ 'X-Org': 'acme', Retries: '3' });
  });

  it('falls back to an empty object for malformed input', () => {
    expect(parseHeaders('')).toEqual({});
    expect(parseHeaders('not json')).toEqual({});
    expect(parseHeaders('[1,2]')).toEqual({});
  });
});

describe('maskSecret', () => {
  it('keeps only the last characters visible', () => {
    const masked = maskSecret('sk-router-1234567890');
    expect(masked.endsWith('7890')).toBe(true);
    expect(masked).not.toContain('123456');
  });
});

describe('formatLatency', () => {
  it('uses milliseconds below one second', () => {
    expect(formatLatency(850)).toBe('850 ms');
  });

  it('uses seconds above one second', () => {
    expect(formatLatency(1500)).toBe('1.50 s');
  });

  it('renders zero and missing values as a dash', () => {
    expect(formatLatency(0)).toBe('—');
  });
});

describe('formatCost', () => {
  it('renders zero as $0.00', () => {
    expect(formatCost(0)).toBe('$0.00');
  });

  it('keeps more precision for sub-dollar amounts', () => {
    // Locale-dependent separators: accept `0.0012` or `0,0012`.
    expect(formatCost(0.001234)).toMatch(/0[.,]0012/);
  });
});

describe('successRate', () => {
  it('formats a percentage', () => {
    expect(successRate(3, 4)).toBe('75.0%');
  });

  it('renders no data as a dash', () => {
    expect(successRate(0, 0)).toBe('—');
  });
});

describe('formatDateTime', () => {
  it('renders missing values as a dash', () => {
    expect(formatDateTime(null)).toBe('—');
    expect(formatDateTime(undefined)).toBe('—');
  });
});
