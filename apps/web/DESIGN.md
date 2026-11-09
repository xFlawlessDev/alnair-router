# Alnair: the router desk

## Direction

Retro editorial collage, based on the user's three screenshots from 2026-11-10: warm paper, cobalt cutouts, navy ink, white sticker edges, torn paper and restrained halftone. Keep the existing Alnair mark and navigation. The reference photographs are not product assets.

ENERGY 3 / RHYTHM 3 / MOTION 1, on the antislop three-level scale. Large, left-aligned display headlines and a strong navy masthead provide energy. Content-specific layouts and numbered directory entries provide rhythm. Motion only acknowledges interactions and existing live activity, with reduced-motion support.

## Tokens and type

- Light: paper `#f4eedf`, panel `#fffaf0`, ink `#15283c`, cobalt `#2448a6`. Muted text `#536173` remains readable on paper.
- Dark: paper `#101c2b`, panel `#192b3e`, ink `#f4eedf`, cobalt highlight `#9db8ff`. Muted text `#b5c2cf`.
- Mustard `#ecc45b` is a highlight backing with navy text; vermilion is reserved for annotations and destructive semantics. Success and warning use their own semantic tokens with text/icons.
- Display: `Arial Black`, `Arial`, sans-serif; heavy, tight, selectively italic to reflect the bold printed reference. Body: system sans-serif for compact readable admin controls. Existing monospace is for technical identifiers only. No external font requests.
- Page heading: fluid 2.25rem to 4rem. Section heading: 1.125rem to 1.5rem. Body: 14px minimum for primary content. 12px for secondary metadata only.
- Spacing: 4/8/12/16/24/32px. Dense tables get more readable rhythm through grouping, not arbitrarily smaller type.
- Radius: 4px base; 8px larger panels; no all-pill treatment. Borders: visible ink-derived lines. Offset shadow only for selected stickers and lead panels, not every card.

The semantic variables in `src/styles.css` are authoritative. Use `bg-background`, `bg-card`, `text-foreground`, `text-muted-foreground`, `border-border`, and existing status tokens for ordinary content.

## Shared application classes

These classes are application styles, without changing the primitive component API:

| Class                                    | Purpose                                                                     |
| ---------------------------------------- | --------------------------------------------------------------------------- |
| `editorial-page`                         | Route-level stack with consistent gap and shrink-safe width.                |
| `editorial-panel`                        | Clean paper panel, defined border, no default floating shadow. Use on Card. |
| `editorial-lead`                         | Lead panel with a thicker top rule for the most important content.          |
| `editorial-section-title`                | Heavy, compact section heading.                                             |
| `editorial-kicker`                       | Small bold rubric label; normal tracking, no tiny letterspaced text.        |
| `editorial-sticker`                      | Small paper label with white edge and offset ink shadow. Static by default. |
| `editorial-sticker--accent`              | Mustard label with dark ink for one selected highlight.                     |
| `editorial-sticker--status`              | Larger status sticker for the Overview health focal point.                  |
| `editorial-toolbar`                      | Wrapping clean control strip.                                               |
| `editorial-table`                        | Class on Card/container for readable table rules and headings.              |
| `editorial-directory`                    | Class on a grid/list container for directory composition.                   |
| `editorial-savebar`                      | Opaque save surface with visible border and offset shadow.                  |
| `editorial-code`                         | Readable code surface with local overflow.                                  |
| `editorial-number`                       | Large tabular display number.                                               |
| `editorial-warning`, `editorial-success` | Theme-aware readable text for semantic status, paired with text or icons.   |

`PageHeader` keeps its existing title/description/actions API. It supplies the shared navy editorial masthead, white title, paper action strip, and torn bottom edge. Do not duplicate it with a second decorative hero. The app shell supplies paper texture only in outer margins; panel content stays clear.

## Composition and accessibility

Use one focal point per page. Maximum two decorative sticker accents in one viewport. Do not rotate inputs, charts, table cells, operational labels, or action buttons. Decorative surfaces use pointer-events none and aria-hidden where appropriate. Use CSS-native texture/edges; no new logo, mascot, stock people, or invented usage data.

Directory pages emphasize names, destinations, status and actions in that order. Monitoring emphasizes counts, failures, then detailed activity. Forms keep visible labels and predictable alignment. Content pages use editorial rules and heading hierarchy. Public pages use the same type and paper system without the admin sidebar.

Use full-width wrapping toolbars on phones, adequate control hit areas, accessible names for icons, local scroll for wide tables/code, and compact views or prioritized columns where appropriate. Preserve access to all details and actions. Validate 320/375/768/1440px, 200% zoom, both themes, long data, empty/loading/error, and keyboard interaction.

Existing Lucide glyphs communicate actions and domain concepts; retain them for recognition rather than replacing them with unrelated retro art. Existing status badges describe actual state. Texture and halftone carry the requested print identity; no decorative dots behind data. The existing topology dot grid communicates graph space. Every chart and table must remain readable in both themes.

## Usage stickers and share images

The Usage and My Usage charts use paper cutout frames, bordered bars, and model legend stickers. Provider topology uses a cobalt router sticker and paper provider stickers; these outlines represent graph nodes rather than decorative labels. Axis labels use the theme's muted text token. The first chart series follows the primary token so it remains visible on dark paper.

My Usage provides a downloadable usage snapshot with two formats: landscape 1200 x 630 for feeds and square 1080 x 1080. Export images keep a light paper palette in either app theme so the preview and file are identical. Total tokens dominate; requests, average tokens per upstream request, recorded USD spend, and four top models supply supporting detail. Models are ranked by prompt plus completion tokens; percentages use the key's total tokens for the same API response window. Additional model count is shown when relevant. No activity-day count, plan value, or comparison is inferred from missing data.

The export explicitly selects name, period, usage totals and model usage. Credentials, key prefixes and router addresses are not serialized. PNG generation is local using embedded SVG and native canvas, without remote assets. Sharing is initiated by the user's click; unsupported file sharing falls back to download and user cancellation does not download a file.

## Ownership

Coordinator owns this file, global CSS, layout and root shared components. Agent A owns management pages/features. Agent B owns monitoring pages/features and usage components used by PublicUsage. Agent C owns configuration, playground and editorial/public pages/features. Request shared changes through the owner; see `docs/UI_REVAMP_DELEGATION_PLAN.md`.
