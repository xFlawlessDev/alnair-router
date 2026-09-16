# Provider icons

The `*.svg` files in this folder come from
[`@lobehub/icons-static-svg`](https://github.com/lobehub/lobe-icons)
v1.95.0 (MIT), used under that license. Each file is
named after the router's provider preset id, not the upstream file name.

Refresh them with `node scripts/sync-provider-icons.mjs` from `apps/web`.
Providers without an icon render a monogram tile instead.
