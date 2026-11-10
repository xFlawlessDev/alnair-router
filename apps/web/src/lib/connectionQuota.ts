import type { Connection } from "@/types/api";

export function hasConnectionQuota(connection: Connection): boolean {
  return (
    connection.provider_type === "openai-codex" ||
    connection.provider_id === "tavily"
  );
}
