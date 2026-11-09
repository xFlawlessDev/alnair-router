# Alnair Router landing page

Static product site using the dashboard's retro editorial and sticker theme.
HTML, CSS, browser JavaScript, and Node's standard library. No dependencies,
backend, provider credentials, or external fonts are needed.

## Local preview

From the repository root, with Node 22 or later:

```sh
node apps/landing/scripts/build.mjs
node apps/landing/scripts/serve.mjs
```

Open `http://127.0.0.1:4174`. Rebuild and refresh after editing. Set `PORT` to
change the preview port. `npm run dev` inside `apps/landing` also builds and serves.
Run `npm run check` there to build and validate metadata, canonical URLs,
sitemap, robots rules, verification-file integrity, breadcrumbs, and CSP hashes.

The build copies the existing shared brand assets and selected images from
`docs/screenshots` into `dist/assets`. Run from a full repository checkout.
The screenshot figures explicitly identify their usage as sample data.

## Cloudflare Pages

Connect this repository in Cloudflare Pages and use:

| Setting                | Value                    |
| ---------------------- | ------------------------ |
| Framework preset       | None                     |
| Root directory         | `apps/landing`           |
| Build command          | `node scripts/build.mjs` |
| Build output directory | `dist`                   |
| Environment variable   | `NODE_VERSION=22`        |

The parent repository remains available to the build for shared assets.
This is a static site; Pages Functions, Workers bindings, and router deployment
are not needed. The top-level `404.html` preserves actual 404 responses instead
of enabling Pages' default SPA fallback.

For build watch paths, include `apps/landing/*`, `assets/*`,
`docs/screenshots/*`, and `apps/web/public/favicon.*` because the build copies
those shared files. Alternatively, keep the default of building for every change.

Sources: [Cloudflare static HTML guide](https://developers.cloudflare.com/pages/framework-guides/deploy-anything/),
[monorepo configuration](https://developers.cloudflare.com/pages/configuration/monorepos/).

Add `router.alnairlabs.com` under the Pages project's Custom domains settings.
Canonical URLs, the sitemap, social images, and JSON-LD use
`https://router.alnairlabs.com/` as the production origin.
Install commands and source links point to the existing public GitHub repository.

## SEO and Google Search Console

The homepage targets self-hosted LLM routers, AI API gateways, OpenAI-compatible
proxies, Anthropic API routing, and multi-provider fallback. `/compare/` answers
the specific Alnair Router vs 9Router vs OmniRoute comparison with links to the
projects' documentation. Claims avoid unmeasured performance rankings and
changing provider/model counts.

Both pages render their content and metadata in the initial HTML. They have
distinct titles/descriptions, canonical URLs, Open Graph and Twitter image
metadata, and JSON-LD. The root identifies the site and open-source software;
the comparison supplies a breadcrumb list. No invented review ratings are used.

`public/robots.txt` allows crawling and points at `sitemap.xml`. The sitemap
contains only `/` and `/compare/`. Google verification and error pages are not
content entries. `_headers` adds `noindex` to both the default `*.pages.dev`
hostname and preview `*.*.pages.dev` hostnames, leaving the custom domain
indexable. The 404 page also has a `noindex` meta tag.

The build copies `public/google8aa23d08e9851d7f.html` unchanged. After deploying:

1. Open `https://router.alnairlabs.com/google8aa23d08e9851d7f.html`. It must return
   HTTP 200 with `google-site-verification: google8aa23d08e9851d7f.html`.
2. Add the URL-prefix property `https://router.alnairlabs.com/` in
   [Google Search Console](https://search.google.com/search-console). Select
   HTML file verification and verify the supplied file. Keep the file deployed
   after verification. If Google issues a different file for this property,
   add that exact file to `public/` and redeploy.
3. Submit `https://router.alnairlabs.com/sitemap.xml` under Sitemaps.
4. Use URL Inspection to test the live homepage and `/compare/`, inspect the
   selected canonical, and request indexing for those URLs.
5. Check Page indexing and Performance after Google has crawled the site. Keep
   the custom domain accessible without a login or bot challenge.

The verification file proves ownership; it does not submit pages for indexing.
Indexing, search appearance, and placement beside competitors are decided by
Google. Deployment and Search Console actions have not been performed locally.

References: [Google canonical URLs](https://developers.google.com/search/docs/crawling-indexing/consolidate-duplicate-urls),
[sitemap submission](https://developers.google.com/search/docs/crawling-indexing/sitemaps/build-sitemap),
[ownership verification](https://support.google.com/webmasters/answer/9008080),
[requesting a crawl](https://developers.google.com/search/docs/crawling-indexing/ask-google-to-recrawl),
[Cloudflare hostname header rules](https://developers.cloudflare.com/pages/configuration/headers/).

## Content and behavior

- `src/index.html`: product content, real dashboard screenshots, install guide.
- `src/compare/index.html`: sourced comparison with 9Router and OmniRoute.
- `src/styles.css`: shared paper/navy/cobalt palette and responsive layouts.
- `src/main.js`: screenshot selector, accessible image dialog, installation
  selector, and clipboard action with a manual-copy fallback.
- `src/_headers`: response headers for Cloudflare Pages.
- `public/`: robots.txt, sitemap, and the Google ownership-verification file.
- `DESIGN.md`: visual decisions and source of product claims.

There are no tracking scripts or requests to a router/provider. With JavaScript
disabled, the overview, content, npm command, and external install guide still
work. Other interactive screenshot and installation controls need JavaScript.
