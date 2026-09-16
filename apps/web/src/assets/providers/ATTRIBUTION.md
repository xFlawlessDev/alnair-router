# Provider icons

Most `*.svg` files in this folder come from
[`@lobehub/icons-static-svg`](https://github.com/lobehub/lobe-icons)
v1.95.0 (MIT), used under that license. Each file is
named after the router's provider preset id, not the upstream file name.

The remaining glyphs are the vendors' own published brand assets, vendored by
hand in `scripts/provider-icons-extra/` because LobeHub does not carry them.
They are used only to identify the provider:

- `bazaarlink.svg` — [bazaarlink.ai](https://bazaarlink.ai) (site favicon, traced by the vendor)
- `blackbox.svg` — [docs.blackbox.ai](https://docs.blackbox.ai) (site favicon)
- `chutes.svg` — [chutesai/chutes-style](https://github.com/chutesai/chutes-style) (official brand mark)
- `commandcode.svg` — [commandcode.ai](https://commandcode.ai) (site masked-icon)
- `kimchi.svg` — [kimchi.dev](https://app.kimchi.dev) (site favicon)

Refresh everything with `node scripts/sync-provider-icons.mjs` from `apps/web`.
Providers without an icon render a monogram tile instead.
