// Vendors the provider brand glyphs the dashboard ships.
//
// Source: @lobehub/icons-static-svg (MIT). The icons are written under our own
// preset ids so ProviderIcon.vue needs no name translation at runtime, and the
// mono variant is used because it fills with `currentColor` — which is what
// makes the glyphs work in both themes once they are inlined into the DOM.
//
// Run from apps/web after `pnpm install`:  node scripts/sync-provider-icons.mjs

import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const packageRoot = join(here, '..', 'node_modules', '@lobehub', 'icons-static-svg');
const sourceDir = join(packageRoot, 'icons');
const targetDir = join(here, '..', 'src', 'assets', 'providers');

/** Size cap: LobeHub ships a few multi-megabyte outliers we do not need. */
const MAX_BYTES = 24 * 1024;

/** Preset id → icon file name inside the package. Anything absent falls back
 *  to a monogram tile in the UI. */
const ICONS = {
  alibaba: 'alibaba',
  'alibaba-coding': 'alibabacloud',
  'alibaba-studio': 'bailian',
  'alibaba-token-plan': 'alibabacloud',
  anthropic: 'anthropic',
  'azure-openai': 'azureai',
  qianfan: 'baidu',
  byteplus: 'bytedance',
  cerebras: 'cerebras',
  cloudflare: 'cloudflare',
  cohere: 'cohere',
  deepseek: 'deepseek',
  featherless: 'featherless',
  fireworks: 'fireworks',
  gemini: 'gemini',
  'glm-china': 'zhipu',
  'glm-coding': 'zai',
  groq: 'groq',
  hyperbolic: 'hyperbolic',
  'kilo-gateway': 'kilocode',
  lmstudio: 'lmstudio',
  minimax: 'minimax',
  'minimax-china': 'minimax',
  mistral: 'mistral',
  moonshot: 'moonshot',
  morph: 'morph',
  nebius: 'nebius',
  nvidia: 'nvidia',
  ollama: 'ollama',
  'ollama-cloud': 'ollama',
  openai: 'openai',
  'opencode-free': 'opencode',
  'opencode-go': 'opencode',
  openrouter: 'openrouter',
  perplexity: 'perplexity',
  poolside: 'poolside',
  siliconflow: 'siliconcloud',
  tencent: 'hunyuan',
  together: 'together',
  venice: 'venice',
  'vercel-ai-gateway': 'vercel',
  vllm: 'vllm',
  'volcengine-ark': 'volcengine',
  xai: 'xai',
  'xiaomi-mimo': 'xiaomimimo',
  'xiaomi-token-plan': 'xiaomimimo',
  zai: 'zai',
};

if (!existsSync(sourceDir)) {
  throw new Error(`no icons at ${sourceDir} — run \`pnpm install\` in apps/web first`);
}

const manifest = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
mkdirSync(targetDir, { recursive: true });

const written = [];
const skipped = [];
const missing = [];

for (const [presetId, iconName] of Object.entries(ICONS)) {
  const source = join(sourceDir, `${iconName}.svg`);

  let svg;
  try {
    svg = readFileSync(source, 'utf8');
  } catch {
    missing.push(`${presetId} (${iconName}.svg)`);
    continue;
  }

  if (Buffer.byteLength(svg) > MAX_BYTES) {
    skipped.push(`${presetId} (${iconName}.svg, ${Buffer.byteLength(svg)} bytes)`);
    continue;
  }

  writeFileSync(join(targetDir, `${presetId}.svg`), svg);
  written.push(presetId);
}

// Drop icons whose preset is gone, so the folder always mirrors the mapping.
for (const file of readdirSync(targetDir)) {
  if (file.endsWith('.svg') && !written.includes(file.replace(/\.svg$/, ''))) {
    rmSync(join(targetDir, file));
  }
}

writeFileSync(
  join(targetDir, 'ATTRIBUTION.md'),
  `# Provider icons

The \`*.svg\` files in this folder come from
[\`@lobehub/icons-static-svg\`](https://github.com/lobehub/lobe-icons)
v${manifest.version} (${manifest.license}), used under that license. Each file is
named after the router's provider preset id, not the upstream file name.

Refresh them with \`node scripts/sync-provider-icons.mjs\` from \`apps/web\`.
Providers without an icon render a monogram tile instead.
`,
);

console.log(`wrote ${written.length} icons to src/assets/providers`);
if (skipped.length) {
  console.log(`skipped (over ${MAX_BYTES} bytes): ${skipped.join(', ')}`);
}
if (missing.length) {
  console.log(`missing from the package: ${missing.join(', ')}`);
}
