import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const root = new URL("../", import.meta.url);
const read = (path) => readFile(new URL(path, root), "utf8");
const origin = "https://router.alnairlabs.com";
const pages = [
  ["index.html", "/"],
  ["compare/index.html", "/compare/"],
];

for (const [file, pathname] of pages) {
  test(`${pathname} exposes crawlable content and consistent URL metadata`, async () => {
    const html = await read(`dist/${file}`);
    assert.equal([...html.matchAll(/<title>/g)].length, 1);
    assert.equal([...html.matchAll(/<h1\b/g)].length, 1);
    assert.equal([...html.matchAll(/rel="canonical"/g)].length, 1);
    assert.match(html, /<html lang="en">/);
    assert.match(
      html,
      /name="robots"\s+content="index, follow, max-image-preview:large"/,
    );
    assert.match(html, /name="description"\s+content="[^"]+"/);
    assert.match(html, /<main\b/);
    assert.match(html, /self-hosted|Self-hosted|Self-host/);
    const canonical = html.match(/rel="canonical"\s+href="([^"]+)"/)[1];
    const ogUrl = html.match(/property="og:url"\s+content="([^"]+)"/)[1];
    assert.equal(canonical, `${origin}${pathname}`);
    assert.equal(ogUrl, canonical);
    assert.match(html, /name="twitter:card"\s+content="summary_large_image"/);
    const image = html.match(/property="og:image"\s+content="([^"]+)"/)[1];
    assert.equal(new URL(image).origin, origin);
    assert(
      (await readFile(new URL(`dist${new URL(image).pathname}`, root))).length >
        0,
    );
    const data = JSON.parse(
      html.match(
        /<script type="application\/ld\+json">([\s\S]*?)<\/script>/,
      )[1],
    );
    assert.equal(data["@context"], "https://schema.org");
    const webpage = data["@graph"].find((item) => item["@type"] === "WebPage");
    assert.equal(webpage.url, canonical);
    assert(!JSON.stringify(data).includes("aggregateRating"));
    assert(!JSON.stringify(data).includes('"review"'));
    assert(!html.includes('name="keywords"'));
  });
}

test("sitemap lists only canonical content pages and robots makes it discoverable", async () => {
  const sitemap = await read("dist/sitemap.xml");
  const locations = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map(
    (match) => match[1],
  );
  assert.deepEqual(
    locations,
    pages.map(([, pathname]) => `${origin}${pathname}`),
  );
  assert.match(
    sitemap,
    /xmlns="http:\/\/www.sitemaps.org\/schemas\/sitemap\/0.9"/,
  );
  assert(!sitemap.includes("<lastmod>"));
  const robots = await read("dist/robots.txt");
  assert.match(robots, /User-agent: \*/);
  assert.match(robots, /^Allow: \/$/m);
  assert(!/^Disallow: \/$/m.test(robots));
  assert(robots.includes(`Sitemap: ${origin}/sitemap.xml`));
});

test("Google verification is copied unchanged and excluded from content URLs", async () => {
  const filename = "google8aa23d08e9851d7f.html";
  const source = await readFile(new URL(`public/${filename}`, root));
  const built = await readFile(new URL(`dist/${filename}`, root));
  assert.deepEqual(built, source);
  assert.equal(
    built.toString().trim(),
    `google-site-verification: ${filename}`,
  );
  assert(!(await read("dist/sitemap.xml")).includes(filename));
});

test("CSP allows the exact JSON-LD blocks without unsafe-inline", async () => {
  const headers = await read("dist/_headers");
  for (const [page] of pages) {
    const html = await read(`dist/${page}`);
    const json = html.match(
      /<script type="application\/ld\+json">([\s\S]*?)<\/script>/,
    )[1];
    const hash = createHash("sha256").update(json).digest("base64");
    assert(headers.includes(`'sha256-${hash}'`));
  }
  assert(!headers.includes("unsafe-inline"));
  const globalRules = headers.split(/\r?\n\r?\n/)[0];
  assert(!globalRules.includes("X-Robots-Tag"));
  assert.match(
    headers,
    /https:\/\/:project.pages.dev\/\*\s+X-Robots-Tag: noindex/,
  );
  assert.match(
    headers,
    /https:\/\/:version.:project.pages.dev\/\*\s+X-Robots-Tag: noindex/,
  );
  assert.match(await read("dist/404.html"), /name="robots" content="noindex"/);
});

test("comparison has sources, breadcrumbs, and a crawlable link from the homepage", async () => {
  const home = await read("dist/index.html");
  const comparison = await read("dist/compare/index.html");
  assert(home.includes('href="/compare/"'));
  assert(comparison.includes('href="/#install"'));
  assert(comparison.includes('href="https://github.com/decolua/9router"'));
  assert(
    comparison.includes(
      'href="https://github.com/cheaper-inference/OmniRoute"',
    ),
  );
  const data = JSON.parse(
    comparison.match(
      /<script type="application\/ld\+json">([\s\S]*?)<\/script>/,
    )[1],
  );
  const breadcrumbs = data["@graph"].find(
    (item) => item["@type"] === "BreadcrumbList",
  );
  assert.deepEqual(
    breadcrumbs.itemListElement.map((item) => item.item),
    pages.map(([, pathname]) => `${origin}${pathname}`),
  );
});
