import type { Connection } from "@/types/api";

export function connectionCheckHint(connection: Connection): string {
  if (connection.provider_id === "tavily")
    return "Check credentials and usage via GET /usage";
  if (connection.provider_id === "firecrawl")
    return "Check credentials and credits via GET /team/credit-usage";
  if (
    connection.search_tier != null ||
    connection.search_path ||
    connection.provider_id === "exa"
  ) {
    return `Check search via POST ${connection.search_path ?? "/search"} (may use credits)`;
  }
  return "Check the provider model API";
}
