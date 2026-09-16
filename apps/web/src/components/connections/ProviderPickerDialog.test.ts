import { createApp, h, nextTick, ref } from 'vue';
import { describe, expect, it, vi } from 'vitest';

import ProviderPickerDialog from './ProviderPickerDialog.vue';
import { api } from '@/lib/api';
import type { ProviderPreset } from '@/types/api';

vi.mock('@/lib/api', () => ({
  ApiError: class ApiError extends Error {},
  api: { listProviders: vi.fn() },
}));

/** Identifies a fixture; everything else falls back to a default. */
type PresetIdentity = Pick<ProviderPreset, 'id' | 'label' | 'base_url' | 'category'>;

const preset = (overrides: PresetIdentity & Partial<ProviderPreset>): ProviderPreset => ({
  provider_type: 'openai-compatible',
  auth: 'api_key',
  default_headers: {},
  api_key_url: null,
  docs_url: null,
  note: null,
  configured: 0,
  ...overrides,
});

const presets: ProviderPreset[] = [
  preset({
    id: 'openai',
    label: 'OpenAI',
    base_url: 'https://api.openai.com/v1',
    category: 'api_key',
  }),
  preset({
    id: 'opencode-free',
    label: 'OpenCode Free',
    base_url: 'https://opencode.ai/zen/v1',
    category: 'free_tier',
    auth: 'none',
  }),
  preset({
    id: 'ollama',
    label: 'Ollama',
    base_url: 'http://localhost:11434/v1',
    category: 'local',
    auth: 'none',
  }),
];

/** Lets the picker's async provider load settle. */
const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const mountPicker = async () => {
  const open = ref(false);
  const selected = vi.fn();

  const app = createApp({
    setup() {
      return () =>
        h(ProviderPickerDialog, {
          open: open.value,
          'onUpdate:open': (value: boolean) => {
            open.value = value;
          },
          onSelect: selected,
        });
    },
  });

  const container = document.createElement('div');
  document.body.appendChild(container);
  app.mount(container);

  open.value = true;
  await settle();

  return {
    selected,
    cleanup: () => {
      app.unmount();
      container.remove();
    },
  };
};

const searchFor = async (term: string) => {
  const input = document.querySelector<HTMLInputElement>('input[aria-label="Search providers"]');
  expect(input, 'search input should render').toBeTruthy();
  input!.value = term;
  input!.dispatchEvent(new Event('input', { bubbles: true }));
  await settle();
};

describe('ProviderPickerDialog', () => {
  it('groups presets into tiers and marks keyless ones', async () => {
    vi.mocked(api.listProviders).mockResolvedValue({
      object: 'list',
      data: presets,
    });
    const { cleanup } = await mountPicker();

    const text = document.body.textContent ?? '';
    expect(text).toContain('API key providers');
    expect(text).toContain('Free tier providers');
    expect(text).toContain('Local servers');
    expect(text).toContain('OpenAI');
    expect(text).toContain('OpenCode Free');
    expect(text).toContain('Ollama');
    expect(text).toContain('no key');

    for (const label of ['OpenAI', 'OpenCode Free', 'Ollama']) {
      const card = [...document.querySelectorAll('button')].find((button) => button.textContent?.includes(label));
      expect(card?.querySelector('svg'), `${label} should render its brand glyph`).toBeTruthy();
    }

    cleanup();
  });

  it('drops tiers that have no match for the search term', async () => {
    vi.mocked(api.listProviders).mockResolvedValue({
      object: 'list',
      data: presets,
    });
    const { cleanup } = await mountPicker();

    await searchFor('ollama');

    const text = document.body.textContent ?? '';
    expect(text).toContain('Local servers');
    expect(text).not.toContain('API key providers');
    expect(text).not.toContain('Free tier providers');

    cleanup();
  });

  it('emits the picked preset', async () => {
    vi.mocked(api.listProviders).mockResolvedValue({
      object: 'list',
      data: presets,
    });
    const { selected, cleanup } = await mountPicker();

    const card = [...document.querySelectorAll('button')].find((button) =>
      button.textContent?.includes('OpenCode Free'),
    );
    expect(card, 'preset card should render').toBeTruthy();

    card!.click();
    await nextTick();

    expect(selected).toHaveBeenCalledWith(expect.objectContaining({ id: 'opencode-free' }));

    cleanup();
  });
});
