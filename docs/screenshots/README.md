# Dashboard screenshots

Captured from a live router at `http://127.0.0.1:7878` with the embedded
production dashboard, at a 1440×900 viewport (2× device pixel ratio → 2880×1800
images). Data is real: three connections, four aliases, a minted API key, and
recorded usage.

| # | Screenshot | Page |
|---|---|---|
| 01 | `01-login.png` | Sign in (`/login`) — password gate, no username |
| 02 | `02-overview.png` | Overview (`/`) — status, usage rollup, live traffic |
| 03 | `03-connections.png` | Connections (`/connections`) — upstreams, masked keys, round-robin key counts |
| 04 | `04-aliases.png` | Aliases (`/aliases`) — prefixes, model overrides |
| 05 | `05-combos.png` | Combos (`/combos`) — named fallback chains (empty state) |
| 06 | `06-keys.png` | API Keys (`/keys`) — minted keys, rules, plans & templates |
| 07 | `07-pricing.png` | Pricing (`/pricing`) — catalog status, model rate table |
| 08 | `08-token-saving-savings.png` | Token Saving (`/token-saver`) — measured vs estimated savings |
| 09 | `09-token-saving-config.png` | Token Saving → Configuration — pipeline saver toggles and levels |
| 10 | `10-playground-chat.png` | Playground → Chat (`/playground`) — streamed completion with tier badge |
| 11 | `11-playground-token-saver.png` | Playground → Token Saver — per-step pipeline results |
| 12 | `12-usage.png` | Usage (`/usage`) — live provider topology and trend chart |
| 13 | `13-console.png` | Console (`/logs`) — live API transcript log |
| 14 | `14-settings.png` | Settings (`/settings`) — runtime configuration overrides |
| 15 | `15-guide.png` | API Guide (`/guide`) — quick start and model references |
| 16 | `16-my-usage.png` | My Usage (`/me`) — self-service usage for a client key |

## Notes

- The API Keys page shows key `001` masked (`sk-router-04c054f2…`); connection
  API keys are masked the same way. No full credential appears in any image.
- `05-combos.png` is the empty state; this router has no combos configured.
- `16-my-usage.png` is signed in with key `001` and shows its real rollup —
  requests, tokens, spend, the budget panel and the per-model trend chart.
- Usage attribution requires `server.require_api_key = true`; with it off,
  `/v1` requests are not tied to a key and `/me` reports an empty rollup. The
  Settings capture reflects this router with the check enabled.
