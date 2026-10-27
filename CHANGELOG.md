# Changelog

All notable user-facing changes to Alnair Router are documented here. Release notes summarize behavior and capabilities rather than listing individual commits.

## [0.1.7](https://github.com/xFlawlessDev/alnair-router/compare/v0.1.6...v0.1.7) (2026-10-27)

### Multimodal chat

- OpenAI-compatible chat requests can include audio, video, and file content parts, alongside text and images. Supported parts are forwarded in their original wire shape.
- Unsupported content parts and malformed media payloads now return a clear `400` error instead of being silently dropped.
- Media input is limited to OpenAI-compatible connections; incompatible provider types are rejected explicitly.

### Web search and fetch

- OpenAI-compatible chat requests can pass `web_search_options` through to providers that support built-in web search.
- Search responses remain provider-specific by default for compatibility. Add `?normalize=true` to `/v1/search` to request Alnair's common result shape.
- Connections can route `/v1/web/fetch` through a configured provider extract endpoint for pages that require JavaScript rendering. Local, SSRF-guarded fetching remains the default.
- The local fetch guard also blocks carrier-grade NAT addresses in `100.64.0.0/10`.

### Dashboard and reliability

- Connection settings expose the optional provider-backed web-fetch mode and extract path.
- Existing connections and clients retain their previous behavior unless they opt into the new features.

## [0.1.6](https://github.com/xFlawlessDev/alnair-router/compare/v0.1.5...v0.1.6) (2026-10-02)

- Improved request accounting when a client cancels an in-flight request.
- Dashboard number fields accept numeric input without requiring string conversion.

## [0.1.5](https://github.com/xFlawlessDev/alnair-router/compare/v0.1.4...v0.1.5) (2026-09-21)

- Connections can choose how upstream credentials are sent, including bearer-token authentication for providers that require it.
- Added OAuth account connections for supported providers, with token refresh and account rotation.
- Improved registration and cleanup for automatic startup on supported desktop platforms.
- Improved the display of recent errors in the dashboard.

## [0.1.4](https://github.com/xFlawlessDev/alnair-router/compare/v0.1.3...v0.1.4) (2026-09-18)

- First-time visitors are guided through setup before reaching the dashboard.
- Added the missing `package-lock.json` ignore rule.

## [0.1.3](https://github.com/xFlawlessDev/alnair-router/compare/v0.1.2...v0.1.3) (2026-09-18)

- Anthropic-compatible connections support extended thinking and prompt caching.
- The provider picker offers quicker setup and custom endpoint options.
- The dashboard includes a repository link and improved alias and combo workflows.
- Improved Windows packaging, cross-platform lifecycle tests, and model-preset selection.

## [0.1.2](https://github.com/xFlawlessDev/alnair-router/compare/v0.1.1...v0.1.2) (2026-09-18)

- Improved reliability of background-process tests across platforms.
- Release commits now include synchronized package manifests.
- Reduced port conflicts in lifecycle tests.

## [0.1.1](https://github.com/xFlawlessDev/alnair-router/compare/v0.1.0...v0.1.1) (2026-09-18)

- Windows binaries include the product icon.
- The router can run in the background instead of holding the terminal open.

## 0.1.0 (2026-09-17)

First public release of Alnair Router, a self-hosted gateway for routing AI requests across providers.

- Route OpenAI-compatible chat and Responses API requests, and Anthropic Messages requests, through a single service.
- Configure provider connections, aliases, and ordered fallback combos, with retries and per-connection limits.
- Proxy embeddings, image, audio, video, moderation, and search requests; protect local web fetching against SSRF.
- Secure credentials and API keys, enforce per-key access and usage limits, and track usage and cost.
- Manage connections, keys, usage, settings, and backups from the embedded dashboard.
- Install and run the service with platform installers, npm packages, Docker, or the command-line interface.
