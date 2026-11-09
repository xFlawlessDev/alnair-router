# Landing page verification

Verified locally on 2026-11-10 using Node 24 and headless Chromium.
Delivery gate: PASS. Production deployment and field performance are not measured.

## Build and browser evidence

- `node apps/landing/scripts/build.mjs`: passes; emits static HTML, CSS, JS,
  shared brand assets, four PNG screenshots, comparison page, sitemap, robots.txt,
  Google verification file, `404.html`, and `_headers`.
- `npm --prefix apps/landing run check`: six SEO contract tests pass.
- `node --check` for the browser script, build script, and preview script: passes.
- Prettier on landing source and scripts: passes.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`:
  pass against the current workspace.
- Browser widths 320, 375, 580, 768, 1024, 1100, 1440, 1920: no page overflow.
- Full-page screenshots reviewed at 375, 768, and 1440px. A 720px viewport
  checks the layout space equivalent to 200% zoom on a 1440px display.
- All referenced assets return HTTP 200; missing paths return HTTP 404.
- No unexpected browser console errors or runtime exceptions.

## SEO verification

| Check                    | Result and evidence                                                                                                                                                                                |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Production origin        | PASS: homepage canonical is `https://router.alnairlabs.com/`; comparison canonical is `/compare/` on the same origin.                                                                              |
| Search content           | PASS: both documents expose their full text, unique title, description, one H1, and structured data without requiring JavaScript.                                                                  |
| Open Graph / Twitter     | PASS: absolute URLs point to the production origin and an existing dashboard image; image dimensions and sample-data alt text are supplied.                                                        |
| Sitemap                  | PASS: valid XML lists exactly the two canonical content URLs; no error pages, verification files, anchors, or fabricated modification dates.                                                       |
| Robots                   | PASS: allows crawling and references the absolute sitemap URL.                                                                                                                                     |
| Preview indexing         | PASS: `_headers` has hostname rules for both `*.pages.dev` and `*.*.pages.dev`; the global rules leave the production custom domain indexable. Hostname rules require deployment to exercise.      |
| Google verification      | PASS: original file, `public/` file, and built file have identical SHA-256 `0030d9f9f31993932a840b8e1d644fc7b82583c0f7e36211405d2f55f57b5c1b`. Local HTTP response is 200 and preserves its bytes. |
| Structured data          | PASS: JSON parses; canonical references agree; source-project and breadcrumb data contain no fabricated ratings.                                                                                   |
| CSP                      | PASS: build computes exact hashes for both JSON-LD blocks. Browser exercised the global CSP with no violations; `unsafe-inline` is absent.                                                         |
| Comparison mobile layout | PASS: 320, 375, 580, 768, 1024, 1100, 1440 and 1920px fit. Horizontal table scroll stays local and works with the keyboard.                                                                        |
| Comparison navigation    | PASS: homepage link opens `/compare/`; breadcrumb returns home; install action opens `/#install`.                                                                                                  |
| Comparison sources       | PASS: repository and license links clicked through to HTTP 200. Competitor descriptions come from their official documentation.                                                                    |
| 404                      | PASS: missing URL returns HTTP 404 with a noindex HTML page.                                                                                                                                       |

The production domain has not been deployed or verified in Search Console by
this task. Ownership verification, sitemap submission, URL Inspection, and
actual Google indexing are separate post-deployment steps in README.md.

## Click-through

| Element                            | Observed result                                                              |
| ---------------------------------- | ---------------------------------------------------------------------------- |
| Overview, Connections, Usage       | Each changes the image, alt text, caption, and pressed state.                |
| Expand screenshot, for each image  | Opens the matching full-size screenshot in a modal dialog.                   |
| Close and Escape                   | Close the dialog; Escape returns focus to the expand button.                 |
| Installation method: npm           | Shows npm install command; copy writes that exact text.                      |
| Installation method: Linux / macOS | Shows shell install command; copy writes that exact text.                    |
| Installation method: Windows       | Shows PowerShell install command; copy writes that exact text.               |
| Copy with clipboard denied         | Announces manual-copy instructions and selects the command.                  |
| Inside the router                  | Navigates to `#inside`.                                                      |
| Install and Install Alnair Router  | Both use `#install`; anchor navigation checked.                              |
| Home wordmarks                     | Both use `#top`; anchor navigation checked.                                  |
| Skip to content                    | First Tab reaches the link; Enter moves to `#main`.                          |
| GitHub and Source                  | Open the repository, HTTP 200.                                               |
| Read the quick start               | Opens repository `#quick-start`, HTTP 200.                                   |
| Read the routing rules             | Opens repository `#what-it-does`, HTTP 200.                                  |
| See more of the dashboard          | Opens `tree/main/docs/screenshots`, HTTP 200.                                |
| Download a release binary          | GitHub redirects latest release to `v0.1.14`, HTTP 200 at verification time. |
| Full install guide                 | Opens repository `#install`, HTTP 200.                                       |
| MIT license                        | Opens repository `blob/main/LICENSE`, HTTP 200.                              |
| 404 page: Back to Alnair Router    | Returns to `/`.                                                              |
| Image failure                      | Blocked screenshot displays an error; switching back recovers.               |
| JavaScript disabled                | Product content, overview and npm command render; JS controls stay hidden.   |
| Reduced motion enabled             | Computed scroll behavior changes to `auto`.                                  |

## Hard gate

| Rule | Result and evidence                                                                                                                  |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------ |
| R-02 | PASS: authored copy has no em dashes.                                                                                                |
| R-03 | PASS: eight viewport widths fit; controls use at least 44px targets.                                                                 |
| R-17 | PASS: no customer or performance statistics; screenshots label sample data.                                                          |
| R-18 | PASS: no testimonial section or fictional identities.                                                                                |
| R-23 | PASS: requested existing visual direction; existing logo and dashboard captures reused.                                              |
| R-24 | PASS: all local anchors exist; external link destinations return 200.                                                                |
| R-25 | PASS: ink/paper 12.95:1, muted/paper 5.46:1, panel/cobalt 7.91:1, ink/mustard 9.01:1, light caption/navy 10.40:1.                    |
| R-26 | PASS: gallery, image dialog, selector, clipboard, and navigation tested above.                                                       |
| R-27 | PASS: screenshot load/error and clipboard pending/success/error states exist; no remote data or empty collections.                   |
| R-28 | PASS: no invented FAQ.                                                                                                               |
| R-32 | PASS: native controls, visible focus, skip link, modal Escape and focus restoration.                                                 |
| R-33 | PASS: UI written directly in source; build only copies source and shared assets.                                                     |
| R-34 | PASS: one fixed paper theme; no unverified theme toggle.                                                                             |
| R-35 | PASS: build and recorded click-through completed.                                                                                    |
| R-36 | PASS: product claims traced to root README and dashboard design documentation.                                                       |
| R-37 | PASS: dashboard direction read before implementation; rationale in DESIGN.md.                                                        |
| R-38 | PASS: existing screenshots identify sample data; route diagram labels its illustrated example.                                       |
| R-39 | PASS: document language, landmarks, heading hierarchy, image alternatives, named native controls, focus, cursors and reduced motion. |
| R-40 | PASS: image dimensions supplied; hero image eager and high priority; local deferred JS; no unmeasured performance claims.            |

## Purpose gate

| Rule | Result and evidence                                                                           |
| ---- | --------------------------------------------------------------------------------------------- |
| R-01 | PASS: existing palette; no gradients or glows.                                                |
| R-04 | PASS: shared Alnair mark; no generic decorative icon library.                                 |
| R-06 | PASS: dashboard display type; monospace reserved for technical text.                          |
| R-07 | PASS: clear paper surfaces; no decorative grids.                                              |
| R-08 | PASS: arrows represent request direction in the routing diagram.                              |
| R-09 | PASS: print labels identify screenshots/formats; no decorative status capsules.               |
| R-10 | PASS: no glass surfaces.                                                                      |
| R-12 | PASS: offset shadows identify headline and product cutouts, as documented.                    |
| R-13 | PASS: no glows.                                                                               |
| R-14 | PASS: routing diagram, ruled notes, product spread and install sheet serve different content. |
| R-19 | PASS: interaction feedback only; reduced-motion behavior checked.                             |
| R-22 | PASS: actual product images, with an explicitly illustrated routing sequence.                 |

## Liveliness and quality locks

| Item         | Result and evidence                                                                        |
| ------------ | ------------------------------------------------------------------------------------------ |
| Dials        | PASS: ENERGY 3 / RHYTHM 3 / MOTION 1 documented before implementation.                     |
| Focal points | PASS: headline, routing sequence, usage image, install command.                            |
| Whitespace   | PASS: ruled section boundaries and different spread proportions.                           |
| Accent       | PASS: mustard emphasizes installation and the sticker feature.                             |
| Motif        | PASS: cobalt cutout, white edges, offset ink shadows, torn section edge.                   |
| Design read  | PASS: dashboard palette, type and print identity reused by explicit request.               |
| C-1          | PASS: visual purposes recorded in DESIGN.md.                                               |
| C-2          | PASS: every control has the behavior recorded above.                                       |
| C-3          | PASS: sections explain actual routing, controls, exports and installation.                 |
| C-4          | PASS: responsive range, image failure, clipboard denial, reduced motion and no-JS checked. |
| C-5          | PASS: source-backed product copy and labelled screenshot fixtures.                         |
| R-05         | PASS: content-specific spreads; mobile layout reflows.                                     |
| R-11         | PASS: consistent small control radii; product shots keep print edges.                      |
| R-15         | PASS: action labels name installation, copy, image expansion and documentation.            |
| R-16         | PASS: specific product copy; no fabricated marketing claims.                               |
| R-20         | PASS: dashboard's existing identity continues into the public page.                        |
| R-21         | PASS: fixed light paper chosen for the magazine direction.                                 |
| R-29         | PASS: paper, ink, cobalt, with a mustard highlight.                                        |
| R-30         | PASS: product's own dashboard and assets supply the visual source.                         |
| R-31         | PASS: layout, typography, cutouts, shadows and arrows documented in DESIGN.md.             |

Cloudflare deployment configuration is documented in README.md and checked
against its official static HTML and monorepo guides. `_headers` is prepared
for Pages; deployed response headers and custom-domain behavior require a
production deployment. No Lighthouse score or real-user Web Vitals is claimed.
