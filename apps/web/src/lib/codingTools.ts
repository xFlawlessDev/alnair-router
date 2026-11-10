export type CodingTool =
  "claude" | "codex" | "omp" | "pi" | "cursor" | "cline" | "opencode";

export interface ToolInstructions {
  location: string;
  content: string;
  detail: string;
}

/** Generate a configuration fragment without embedding a client secret. */
export function codingToolInstructions(
  tool: CodingTool,
  routerUrl: string,
  model: string,
): ToolInstructions {
  const root = routerUrl.trim().replace(/\/+$/, "").replace(/\/v1$/, "");
  const baseUrl = `${root}/v1`;
  const q = JSON.stringify;
  switch (tool) {
    case "claude":
      return {
        location: "~/.claude/settings.json",
        detail:
          "Merge the env entries into the existing settings file; replace the key placeholder with a router key.",
        content: JSON.stringify(
          {
            env: {
              ANTHROPIC_BASE_URL: root,
              ANTHROPIC_AUTH_TOKEN: "<router-key>",
              ANTHROPIC_MODEL: model,
            },
          },
          null,
          2,
        ),
      };
    case "codex":
      return {
        location: "~/.codex/config.toml",
        detail:
          "Set ALNAIR_ROUTER_API_KEY in the environment before launching Codex; merge these settings into your existing config.",
        content: `model = ${q(model)}\nmodel_provider = "alnair"\n\n[model_providers.alnair]\nname = "Alnair Router"\nbase_url = ${q(baseUrl)}\nenv_key = "ALNAIR_ROUTER_API_KEY"\nwire_api = "responses"`,
      };
    case "pi":
      return {
        location: "~/.pi/agent/models.json",
        detail:
          "Set ALNAIR_ROUTER_API_KEY in the environment, then merge the provider into models.json and select it in /model.",
        content: JSON.stringify(
          {
            providers: {
              alnair: {
                baseUrl,
                api: "openai-completions",
                apiKey: "${ALNAIR_ROUTER_API_KEY}",
                models: [{ id: model }],
              },
            },
          },
          null,
          2,
        ),
      };
    case "omp":
      return {
        location: "~/.omp/agent/models.yml",
        detail:
          "Set ALNAIR_ROUTER_API_KEY in the environment, then merge the provider into models.yml and select it in /model.",
        content: `providers:\n  alnair:\n    baseUrl: ${q(baseUrl)}\n    api: openai-completions\n    apiKey: ALNAIR_ROUTER_API_KEY\n    models:\n      - id: ${q(model)}`,
      };
    case "opencode":
      return {
        location: "OpenCode configuration (opencode.jsonc)",
        detail:
          "Set ALNAIR_ROUTER_API_KEY in the OpenCode server environment and merge this provider into your configuration.",
        content: JSON.stringify(
          {
            model: "alnair/router",
            providers: {
              alnair: {
                env: ["ALNAIR_ROUTER_API_KEY"],
                package: "@opencode/ai/providers/openai-compatible",
                settings: { baseURL: baseUrl },
                models: { router: { modelID: model } },
              },
            },
          },
          null,
          2,
        ),
      };
    case "cline":
      return {
        location: "Cline settings → API Provider → OpenAI Compatible",
        detail:
          "Enter these values in Cline settings, along with a router-issued API key.",
        content: `Base URL: ${baseUrl}\nModel ID: ${model}\nAPI Key: <router-key>`,
      };
    case "cursor":
      return {
        location: "Cursor Settings → Models",
        detail:
          "Configure the API key and base URL in Cursor; custom API keys are limited to supported standard chat models.",
        content: `OpenAI Base URL: ${baseUrl}\nModel: ${model}\nAPI Key: <router-key>`,
      };
  }
}
