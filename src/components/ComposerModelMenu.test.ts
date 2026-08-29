import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { GROK_BUILD_EFFORTS } from "@/lib/grokCatalog";
import {
  ComposerModelMenu,
  partitionComposerModels,
  resolveComposerEfforts,
} from "./ComposerModelMenu";

const labels = {
  model: "Model",
  effort: "Reasoning",
  effortHigh: "High",
  effortMedium: "Medium",
  effortLow: "Low",
};

describe("partitionComposerModels", () => {
  it("keeps official and custom channels in separate groups", () => {
    const grouped = partitionComposerModels([
      { id: "grok-4.5", label: "Sunsetz 4.5", source: "official" },
      { id: "cldapi", label: "Cldapi · claude-opus-4-6", source: "custom" },
    ]);
    expect(grouped.official.map((item) => item.id)).toEqual(["grok-4.5"]);
    expect(grouped.custom.map((item) => item.id)).toEqual(["cldapi"]);
  });
});

describe("resolveComposerEfforts", () => {
  it("shows only the levels declared by the active live model", () => {
    const efforts = resolveComposerEfforts({
      modelId: "runtime-model",
      models: [
        {
          id: "runtime-model",
          label: "Runtime",
          capabilities: { reasoningEfforts: ["low", "high"] },
        },
      ],
    });
    expect(efforts.map((item) => item.id)).toEqual(["low", "high"]);
  });

  it("hides effort when a live catalog does not declare the capability", () => {
    const efforts = resolveComposerEfforts({
      modelId: "unknown-model",
      models: [{ id: "unknown-model", label: "Unknown" }],
    });
    expect(efforts).toEqual([]);
  });

  it("lets an explicit empty capability override suppress the legacy fallback", () => {
    expect(
      resolveComposerEfforts({
        modelId: "grok-4.5",
        efforts: [],
      }),
    ).toEqual([]);
  });

  it("keeps the old standalone call compatible when no model catalog is passed", () => {
    expect(
      resolveComposerEfforts({ modelId: "grok-4.5" }).map(
        (item) => item.id,
      ),
    ).toEqual(GROK_BUILD_EFFORTS.map((item) => item.id));
  });

  it("does not leak an undeclared effort into the compact trigger", () => {
    const html = renderToStaticMarkup(
      createElement(ComposerModelMenu, {
        modelId: "runtime-model",
        effort: "medium",
        models: [{ id: "runtime-model", label: "Runtime" }],
        labels,
        onModel: () => undefined,
        onEffort: () => undefined,
      }),
    );
    expect(html).toContain("Runtime");
    expect(html).not.toContain("Medium");
  });
});
