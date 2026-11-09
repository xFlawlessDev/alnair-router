import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../dist");
const port = Number(process.env.PORT || 4174);
const headerBlock = (await readFile(resolve(root, "_headers"), "utf8")).split(
  /\r?\n\r?\n/,
)[0];
const globalHeaders = Object.fromEntries(
  headerBlock.split(/\r?\n/).flatMap((line) => {
    const match = line.match(/^\s+([^:]+):\s*(.+)$/);
    return match ? [[match[1], match[2]]] : [];
  }),
);
const types = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".xml": "application/xml; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
};

createServer(async (request, response) => {
  try {
    const url = new URL(request.url, "http://localhost");
    const path = resolve(
      root,
      `.${decodeURIComponent(url.pathname.endsWith("/") ? `${url.pathname}index.html` : url.pathname)}`,
    );
    if (!path.startsWith(`${root}${sep}`)) {
      response.writeHead(403, globalHeaders).end("Forbidden");
      return;
    }
    const body = await readFile(path);
    response.writeHead(200, {
      ...globalHeaders,
      "Content-Type": types[extname(path)] || "application/octet-stream",
    });
    response.end(body);
  } catch {
    const body = await readFile(resolve(root, "404.html")).catch(
      () => "Not found",
    );
    response
      .writeHead(404, {
        ...globalHeaders,
        "Content-Type": "text/html; charset=utf-8",
      })
      .end(body);
  }
}).listen(port, "127.0.0.1", () =>
  console.log(`Landing preview: http://127.0.0.1:${port}`),
);
