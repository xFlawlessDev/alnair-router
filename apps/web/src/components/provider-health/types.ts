import type { ConnectionTestResult } from "@/types/api";

export interface ProbeResult {
  result: ConnectionTestResult | null;
  error: string | null;
  checkedAt: string;
}

export interface HealthDisplayRow {
  name: string;
  detail?: string;
  status: "healthy" | "degraded" | "unhealthy" | "unknown" | "disabled";
  attempts: number;
  failures: number;
  laterAttempts: number;
  p95: number;
  issue?: string;
  connectionId?: string;
}
