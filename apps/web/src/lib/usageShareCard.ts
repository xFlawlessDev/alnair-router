import brandMark from "@assets/alnair-white.svg?raw";
import { formatCompact, formatCost, successRate } from "@/lib/format";
import type { ModelUsage, UsageSummary } from "@/types/api";

export interface UsageShareSnapshot {
  name: string;
  since: string | null;
  until: string | null;
  capturedAt: Date;
  models?: readonly ModelUsage[];
  summary: Pick<
    UsageSummary,
    | "requests"
    | "ok_requests"
    | "prompt_tokens"
    | "completion_tokens"
    | "cost_usd"
  >;
}

export type UsageShareFormat = "square" | "landscape";
export const USAGE_CARD_DIMENSIONS = {
  square: { width: 1080, height: 1080 },
  landscape: { width: 1200, height: 630 },
} as const;

export function rankUsageModels(
  models: readonly ModelUsage[],
  totalTokens: number,
): { model: string; tokens: number; share: number }[] {
  return [...models]
    .filter(
      (model) =>
        model.requests > 0 || model.prompt_tokens + model.completion_tokens > 0,
    )
    .sort(
      (left, right) =>
        right.prompt_tokens +
          right.completion_tokens -
          (left.prompt_tokens + left.completion_tokens) ||
        right.requests - left.requests ||
        left.model.localeCompare(right.model),
    )
    .map((model) => ({
      model: model.model,
      tokens: model.prompt_tokens + model.completion_tokens,
      share:
        totalTokens > 0
          ? ((model.prompt_tokens + model.completion_tokens) / totalTokens) *
            100
          : 0,
    }));
}

function xml(value: string): string {
  return value.replace(/[\u0000-\u0008\u000B\u000C\u000E-\u001F]/g, "").replace(
    /[&<>"']/g,
    (character) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&apos;",
      })[character]!,
  );
}

function shorten(value: string, length: number): string {
  const characters = Array.from(value);
  return characters.length > length
    ? `${characters.slice(0, length - 1).join("")}…`
    : value;
}

function dateLabel(value: Date): string {
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  }).format(value);
}

export function usageCardPeriod(
  snapshot: Pick<UsageShareSnapshot, "since" | "until" | "capturedAt">,
): string {
  if (!snapshot.since) return "All time";
  const start = new Date(snapshot.since);
  const end = snapshot.until ? new Date(snapshot.until) : snapshot.capturedAt;
  if (Number.isNaN(start.getTime()) || Number.isNaN(end.getTime()))
    return "Selected period";
  const formatter = new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    timeZone: "UTC",
  });
  return `${formatter.format(start)} to ${formatter.format(end)} UTC`;
}

/** An explicit field list keeps credentials and router addresses out of the image. */
export function buildUsageShareSvg(
  snapshot: UsageShareSnapshot,
  format: UsageShareFormat = "square",
): string {
  const { summary } = snapshot;
  const tokens = formatCompact(
    summary.prompt_tokens + summary.completion_tokens,
  );
  const requests = formatCompact(summary.requests);
  const spend =
    Math.abs(summary.cost_usd) >= 10_000
      ? new Intl.NumberFormat("en-US", {
          style: "currency",
          currency: "USD",
          notation: "compact",
          maximumFractionDigits: 1,
        }).format(summary.cost_usd)
      : formatCost(summary.cost_usd);
  const period = usageCardPeriod(snapshot);
  const totalTokens = summary.prompt_tokens + summary.completion_tokens;
  const topModels = rankUsageModels(snapshot.models ?? [], totalTokens);
  const average = summary.requests
    ? formatCompact(totalTokens / summary.requests)
    : "N/A";
  const rate = summary.requests
    ? `${successRate(summary.ok_requests, summary.requests)} successful`
    : "No requests in this period";
  const logo = (x: number, y: number, size: number, color: string) =>
    brandMark
      .replace(/<\?xml[^>]*\?>/, "")
      .replace(
        "<svg ",
        `<svg x="${x}" y="${y}" width="${size}" height="${size}" `,
      )
      .replaceAll("#fff", color);
  const numberSize = (value: string, maximum: number, width: number) =>
    Math.min(maximum, width / (Array.from(value).length * 0.68));
  const percent = (value: number) =>
    value > 0 && value < 0.1
      ? "&lt;0.1%"
      : `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)}%`;
  const modelRows = (landscape: boolean) => {
    const x = landscape ? 814 : 92;
    const width = landscape ? 314 : 884;
    const color = landscape ? "#fffaf0" : "#15283c";
    const firstY = landscape ? 154 : 829;
    const step = landscape ? 88 : 45;
    if (!topModels.length)
      return `<text x="${x}" y="${firstY}" font-size="${landscape ? 18 : 22}" fill="${color}">No model usage in this period</text>`;
    return topModels
      .slice(0, 4)
      .map((model, index) => {
        const y = firstY + index * step;
        return `<text x="${x}" y="${y}" font-family="monospace" font-size="${landscape ? 16 : 19}" fill="${color}">${xml(shorten(model.model, landscape ? 25 : 60))}</text>
        <text x="${x + width}" y="${y}" text-anchor="end" font-size="${landscape ? 22 : 20}" font-weight="700" fill="${color}">${percent(model.share)}</text>
        <rect x="${x}" y="${y + 12}" width="${width}" height="7" fill="${landscape ? "#6277b1" : "#ded7c8"}"/>
        <rect x="${x}" y="${y + 12}" width="${(width * Math.min(100, Math.max(0, model.share))) / 100}" height="7" fill="${landscape ? "#ecc45b" : "#2448a6"}"/>`;
      })
      .join("");
  };
  const metric = (
    x: number,
    y: number,
    width: number,
    label: string,
    value: string,
    fill: string,
  ) => `<g>
    <rect x="${x + 6}" y="${y + 7}" width="${width}" height="120" rx="4" fill="#15283c"/>
    <rect x="${x}" y="${y}" width="${width}" height="120" rx="4" fill="${fill}" stroke="#fff" stroke-width="10"/>
    <text x="${x + 18}" y="${y + 34}" font-size="${width < 250 ? 16 : 18}" font-weight="700">${label}</text>
    <text x="${x + 16}" y="${y + 93}" font-family="Arial Black, Arial, sans-serif" font-size="${numberSize(value, width < 250 ? 45 : 54, width - 36)}" font-weight="900">${xml(value)}</text>
  </g>`;
  const dimensions = USAGE_CARD_DIMENSIONS[format];
  const landscape = format === "landscape";
  const content = landscape
    ? `
    ${logo(663, 24, 54, "#15283c")}
    <text x="54" y="62" font-size="25" font-weight="800">ALNAIR / MY API USAGE</text>
    <g transform="rotate(-1 386 134)">
      <rect x="62" y="108" width="648" height="61" rx="4" fill="#15283c"/>
      <rect x="54" y="100" width="648" height="61" rx="4" fill="#fffaf0" stroke="#fff" stroke-width="10"/>
      <text x="76" y="141" font-size="29" font-weight="700">${xml(shorten(snapshot.name || "My API key", 33))}</text>
    </g>
    <g transform="rotate(.6 386 290)">
      <rect x="62" y="200" width="648" height="166" rx="6" fill="#15283c"/>
      <rect x="54" y="192" width="648" height="166" rx="6" fill="#2448a6" stroke="#fff" stroke-width="12"/>
      <text x="78" y="231" font-size="22" font-weight="700" fill="#fffaf0">TOTAL TOKENS</text>
      <text x="72" y="330" font-family="Arial Black, Arial, sans-serif" font-size="${numberSize(tokens, 110, 596)}" font-weight="900" fill="#fffaf0">${xml(tokens)}</text>
    </g>
    ${metric(54, 398, 200, "REQUESTS", requests, "#ecc45b")}
    ${metric(278, 398, 200, "TOKENS / REQUEST", average, "#fffaf0")}
    ${metric(502, 398, 200, "SPEND / USD", spend, "#fffaf0")}
    <text x="56" y="557" font-size="19" font-weight="700">${xml(rate)} · Upstream attempts</text>
    <text x="56" y="589" font-size="${Math.min(17, 650 / (period.length * 0.54))}" fill="#536173">${xml(period)}</text>
    <g transform="rotate(.5 966 305)">
      <rect x="782" y="50" width="378" height="546" rx="6" fill="#15283c"/>
      <rect x="774" y="42" width="378" height="546" rx="6" fill="#2448a6" stroke="#fff" stroke-width="12"/>
      <text x="814" y="94" font-size="25" font-weight="800" fill="#fffaf0">TOP MODELS</text>
      ${modelRows(true)}
      <text x="814" y="523" font-size="16" fill="#fffaf0">${topModels.length > 4 ? `+${topModels.length - 4} more model${topModels.length > 5 ? "s" : ""}` : "By prompt + completion tokens"}</text>
      <text x="814" y="557" font-size="14" fill="#fffaf0">Captured ${xml(dateLabel(snapshot.capturedAt))} UTC</text>
    </g>`
    : `
    <path d="M0 0H1080V222L1032 231 978 221 921 236 865 226 804 238 747 222 692 234 633 225 574 239 510 223 455 234 394 224 338 239 274 225 221 235 162 223 103 238 51 224 0 235Z" fill="#15283c"/>
    <rect width="1080" height="12" fill="#2448a6"/>
    ${logo(922, 42, 92, "#fffaf0")}
    <text x="64" y="70" font-size="26" font-weight="700" fill="#ecc45b">ALNAIR ROUTER / USAGE SNAPSHOT</text>
    <text x="60" y="174" font-family="Arial Black, Arial, sans-serif" font-size="86" font-style="italic" font-weight="900" fill="#fffaf0">MY API USAGE</text>
    <g transform="rotate(-1.4 540 263)">
      <rect x="72" y="236" width="942" height="68" rx="4" fill="#15283c"/>
      <rect x="64" y="228" width="942" height="68" rx="4" fill="#fffaf0" stroke="#fff" stroke-width="12"/>
      <text x="88" y="274" font-size="34" font-weight="700">${xml(shorten(snapshot.name || "My API key", 42))}</text>
    </g>
    <text x="70" y="344" font-size="20" fill="#536173">${xml(period)}</text>
    <g transform="rotate(.7 540 460)">
      <rect x="76" y="374" width="938" height="210" rx="8" fill="#15283c"/>
      <rect x="64" y="362" width="938" height="210" rx="8" fill="#2448a6" stroke="#fff" stroke-width="14"/>
      <text x="104" y="407" font-size="25" font-weight="700" fill="#fffaf0">TOTAL TOKENS</text>
      <text x="96" y="512" font-family="Arial Black, Arial, sans-serif" font-size="${numberSize(tokens, 120, 840)}" font-weight="900" fill="#fffaf0">${xml(tokens)}</text>
      <text x="104" y="550" font-size="24" fill="#fffaf0">${xml(formatCompact(summary.prompt_tokens))} prompt / ${xml(formatCompact(summary.completion_tokens))} completion</text>
    </g>
    ${metric(68, 612, 290, "REQUESTS", requests, "#ecc45b")}
    ${metric(390, 612, 290, "TOKENS / REQUEST", average, "#fffaf0")}
    ${metric(712, 612, 290, "SPEND / USD", spend, "#fffaf0")}
    <rect x="76" y="774" width="934" height="238" rx="4" fill="#15283c"/>
    <rect x="68" y="766" width="934" height="238" rx="4" fill="#fffaf0" stroke="#fff" stroke-width="8"/>
    <text x="92" y="795" font-size="17" font-weight="700" fill="#536173">TOP MODELS / TOKEN SHARE</text>
    ${modelRows(false)}
    ${topModels.length > 4 ? `<text x="976" y="795" text-anchor="end" font-size="16" fill="#536173">+${topModels.length - 4} more</text>` : ""}
    <text x="70" y="1044" font-size="22" font-weight="700">${xml(rate)}</text>
    <text x="1002" y="1044" text-anchor="end" font-size="18" fill="#536173">Captured ${xml(dateLabel(snapshot.capturedAt))} UTC</text>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${dimensions.width}" height="${dimensions.height}" viewBox="0 0 ${dimensions.width} ${dimensions.height}">
  <defs>
    <pattern id="print-dots" width="12" height="12" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r="1" fill="#15283c" opacity=".12"/></pattern>
  </defs>
  <rect width="${dimensions.width}" height="${dimensions.height}" fill="#f4eedf"/>
  <rect x="828" y="270" width="252" height="470" fill="url(#print-dots)"/>
  <g font-family="Arial, sans-serif" fill="#15283c">
    ${content}
  </g>
</svg>`;
}

export function usageShareFile(
  svg: string,
  format: UsageShareFormat = "square",
): Promise<File> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = USAGE_CARD_DIMENSIONS[format].width;
      canvas.height = USAGE_CARD_DIMENSIONS[format].height;
      const context = canvas.getContext("2d");
      if (!context) {
        reject(new Error("Image export is unavailable in this browser"));
        return;
      }
      try {
        context.drawImage(image, 0, 0);
        canvas.toBlob((blob) => {
          if (!blob) {
            reject(new Error("Could not create the PNG image"));
            return;
          }
          resolve(
            new File([blob], `alnair-api-usage-${format}.png`, {
              type: "image/png",
            }),
          );
        }, "image/png");
      } catch (error) {
        reject(error);
      }
    };
    image.onerror = () => reject(new Error("Could not render the usage image"));
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  });
}
