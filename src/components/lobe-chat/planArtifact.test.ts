import { describe, expect, it } from "vitest";
import { buildPlanArtifactPreview } from "./planArtifact";

describe("buildPlanArtifactPreview", () => {
  it("prefers and normalizes the authored plan body", () => {
    const preview = buildPlanArtifactPreview(
      "  # Plan\r\n\r\nDo the work  ",
      [{ content: "fallback", status: "pending" }],
    );
    expect(preview.markdown).toBe("# Plan\n\nDo the work");
    expect(preview.truncated).toBe(false);
  });

  it("falls back to structured entries", () => {
    const preview = buildPlanArtifactPreview("", [
      { content: "Inspect", status: "completed" },
      { title: "Implement", status: "in_progress" },
    ]);
    expect(preview.markdown).toContain("[x] Inspect");
    expect(preview.markdown).toContain("[~] Implement");
  });

  it("returns a bounded preview without discarding full markdown", () => {
    const preview = buildPlanArtifactPreview("one\ntwo\nthree", [], 2);
    expect(preview.preview).toBe("one\ntwo");
    expect(preview.markdown).toBe("one\ntwo\nthree");
    expect(preview.lineCount).toBe(3);
    expect(preview.truncated).toBe(true);
  });

  it("handles an empty artifact", () => {
    expect(buildPlanArtifactPreview("", [])).toEqual({
      markdown: "",
      preview: "",
      lineCount: 0,
      truncated: false,
    });
  });
});
