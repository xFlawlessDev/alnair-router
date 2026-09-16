import { describe, expect, it } from 'vitest';

import { providerIcon, providerIcons, providerInitials, providerTypeIcon } from './providerIcons';

describe('providerIcons', () => {
  it('vendors a real glyph for every bundled provider', () => {
    const ids = Object.keys(providerIcons);

    expect(ids.length).toBeGreaterThan(30);
    for (const id of ids) {
      // Catches an empty or truncated download instead of a blank icon on screen.
      expect(providerIcons[id], id).toContain('<svg');
    }
  });

  it('resolves a preset id and tolerates anything else', () => {
    expect(providerIcon('openai')).toContain('<svg');
    expect(providerIcon('  OpenAI ')).toContain('<svg');
    expect(providerIcon('not-a-provider')).toBeNull();
    expect(providerIcon(null)).toBeNull();
    expect(providerIcon(undefined)).toBeNull();
  });

  it('derives initials for the monogram fallback', () => {
    expect(providerInitials('Groq')).toBe('GR');
    expect(providerInitials('Alibaba Token Plan')).toBe('AT');
    expect(providerInitials('Minimax (China)')).toBe('MC');
    expect(providerInitials('command-code')).toBe('CC');
    expect(providerInitials('   ')).toBe('?');
  });

  it('falls back to the wire family for connections without a preset', () => {
    expect(providerTypeIcon('openai-compatible')).toContain('<svg');
    expect(providerTypeIcon('anthropic-native')).toContain('<svg');
    // No glyph for Command Code yet, so it keeps the monogram.
    expect(providerTypeIcon('command-code')).toBeNull();
    expect(providerTypeIcon(null)).toBeNull();
    expect(providerTypeIcon(undefined)).toBeNull();
  });
});
