import { describe, expect, it } from "vitest";
import { codingToolInstructions } from "./codingTools";

describe("coding tool configuration", () => {
  it("normalizes the router URL and preserves the selected model", () => {
    const instruction = codingToolInstructions(
      "pi",
      "http://127.0.0.1:7878/v1/",
      "coding/one",
    );
    const parsed = JSON.parse(instruction.content);
    expect(parsed.providers.alnair.baseUrl).toBe("http://127.0.0.1:7878/v1");
    expect(parsed.providers.alnair.models[0].id).toBe("coding/one");
  });

  it("uses the Anthropic root for Claude Code", () => {
    const parsed = JSON.parse(
      codingToolInstructions("claude", "http://localhost:7878/v1", "combo")
        .content,
    );
    expect(parsed.env.ANTHROPIC_BASE_URL).toBe("http://localhost:7878");
    expect(parsed.env.ANTHROPIC_MODEL).toBe("combo");
  });

  it("quotes user values in Codex config", () => {
    const content = codingToolInstructions(
      "codex",
      "http://localhost:7878",
      'a"b',
    ).content;
    expect(content).toContain('model = "a\\"b"');
    expect(content).toContain('wire_api = "responses"');
  });

  it("uses an explicit upstream model ID for OpenCode aliases containing slashes", () => {
    const parsed = JSON.parse(
      codingToolInstructions("opencode", "http://localhost:7878", "oa/gpt-4o")
        .content,
    );
    expect(parsed.model).toBe("alnair/router");
    expect(parsed.providers.alnair.models.router.modelID).toBe("oa/gpt-4o");
  });
});
