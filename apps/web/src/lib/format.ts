export function isEnabled(value: number): boolean {
  return value !== 0;
}

export function parseHeaders(json: string): Record<string, string> {
  try {
    const parsed: unknown = JSON.parse(json || '{}');
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed as Record<string, unknown>).map(([key, value]) => [key, String(value)]),
    );
  } catch {
    return {};
  }
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date);
}

export function formatNumber(value: number): string {
  return new Intl.NumberFormat().format(value);
}

export function formatCompact(value: number): string {
  return new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(value);
}

export function formatCost(value: number): string {
  if (!Number.isFinite(value) || value === 0) return '$0.00';
  const maximumFractionDigits = Math.abs(value) < 1 ? 4 : 2;
  return new Intl.NumberFormat(undefined, {
    style: 'currency',
    currency: 'USD',
    maximumFractionDigits,
  }).format(value);
}

export function formatLatency(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—';
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

export function maskSecret(value: string, visible = 4): string {
  if (value.length <= visible) return '••••';
  return `${'•'.repeat(Math.min(value.length - visible, 12))}${value.slice(-visible)}`;
}

export function successRate(ok: number, total: number): string {
  if (!total) return '—';
  return `${((ok / total) * 100).toFixed(1)}%`;
}
