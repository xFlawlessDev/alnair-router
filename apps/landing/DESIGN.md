# The public router desk

Direction comes from `../web/DESIGN.md` and the user's request to reuse the
dashboard style. ENERGY 3 / RHYTHM 3 / MOTION 1.

Warm paper #f4eedf, panel #fffaf0, navy ink #15283c, cobalt #2448a6,
mustard #ecc45b. The site has a fixed light paper theme because it presents the
printed magazine identity. Arial Black supplies the same heavy editorial voice;
system Arial keeps reading clear without remote font requests. Monospace appears
only in API routes and commands. Small print labels identify real image formats
and sample data rather than inventing product status.

The opening spread pairs a large headline with the real dashboard. A cobalt
cutout emphasizes the single address, with a mustard install action. A compact
install snippet under the actions repeats the npm command with its own copy
button, so the first step needs no scrolling. Its copy-status line stays empty
until a copy is attempted. The routing
section uses a diagram because ordered fallback needs a visible sequence; its
arrows communicate request direction. Supporting controls sit in a ruled index,
not repeated feature cards. The usage section shows the actual sharing surface.
The closing cobalt spread separates installation from the product narrative.
Offsets and white borders are reserved for the headline and product cutouts.
A torn edge connects the install spread to the dashboard's printed motif.

Only the headline cutout and the word sticker are angled. Controls and product
images remain level. Motion is limited to interaction feedback and anchor
scrolling; reduced motion turns smooth scrolling off. No decorative icons,
testimonials, customer counts, or invented performance claims appear.

Copy and install commands are sourced from the root README, and usage export
behavior from `../web/DESIGN.md`. Dashboard screenshots come from
`../../docs/screenshots` and visibly identify sample data. The illustrated
fallback diagram uses the README's example combo name, not live provider status.

Desktop uses an asymmetric opening spread. Mid widths keep two columns for
compact routing content while the product shot takes a full row. Phones stack
the reading flow, use a second nav row, and provide a full-size image dialog
with local horizontal scroll. Nothing needs a provider or backend to render.

## Search and comparison content

The homepage names the product category in its opening paragraph and page title:
self-hosted LLM router and AI API gateway. API paths explain actual compatibility.
The `/compare/` reading page addresses evaluation alongside 9Router and OmniRoute.
It uses the same paper, ink, display type and ruled editorial layout, with a table
for like-for-like decisions. The Alnair column has a paper backing so readers can
find this project's implementation without implying a performance ranking.

Comparison claims come from the root README, `docs/HANDOVER.md`, and the official
repositories at `github.com/decolua/9router` and
`github.com/cheaper-inference/OmniRoute`, reviewed on 2026-11-10. Links to these
sources appear beside the comparison. No provider counts, benchmark numbers,
reviews, or star ratings are added. Competitors are mentioned as separate projects,
not as alternate names or identities for Alnair.

Structured data describes the website, the source project, and real breadcrumbs.
It does not invent organization credentials or app reviews. All production URL
signals use `https://router.alnairlabs.com/`, the domain supplied by the user.
