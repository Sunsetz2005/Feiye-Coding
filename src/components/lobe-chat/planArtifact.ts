import { planDisplayMarkdown } from "@/lib/planBody";

export interface PlanArtifactPreview {
  markdown: string;
  preview: string;
  lineCount: number;
  truncated: boolean;
}

/**
 * Normalize the persisted plan body into a compact transcript preview.
 * The full markdown remains available to the resources panel via the caller.
 */
export function buildPlanArtifactPreview(
  body: string | null | undefined,
  entries: unknown[] | null | undefined,
  maxLines = 10,
): PlanArtifactPreview {
  const markdown = planDisplayMarkdown(body, entries)
    .replace(/\r\n?/g, "\n")
    .trim();
  const lines = markdown ? markdown.split("\n") : [];
  const limit = Math.max(1, Math.floor(maxLines));
  const truncated = lines.length > limit;
  return {
    markdown,
    preview: lines.slice(0, limit).join("\n"),
    lineCount: lines.length,
    truncated,
  };
}
