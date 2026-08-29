import { describe, expect, it } from "vitest";
import {
  CONNECTOR_CATALOG,
  connectorById,
  connectorConnectsInApp,
  featuredConnectors,
  productivityConnectors,
} from "./connectorCatalog";

describe("connectorCatalog", () => {
  it("looks up featured and productivity slices by id", () => {
    expect(connectorById("gmail")?.openConnectorSlug).toBe("gmail");
    expect(connectorById("missing")).toBeUndefined();
    expect(featuredConnectors().every((row) => row.category === "featured")).toBe(
      true,
    );
    expect(
      productivityConnectors().every((row) => row.category === "productivity"),
    ).toBe(true);
    expect(CONNECTOR_CATALOG).toHaveLength(
      featuredConnectors().length + productivityConnectors().length,
    );
  });

  it("connects GitHub and Google apps in-app and keeps other catalog apps coming soon", () => {
    const inApp = ["github", "gmail", "google-drive", "google-calendar"];
    for (const id of inApp) {
      const entry = connectorById(id);
      expect(entry?.connectKind).toBe("in_app");
      expect(entry && connectorConnectsInApp(entry)).toBe(true);
    }
    expect(
      CONNECTOR_CATALOG.filter((row) => !inApp.includes(row.id)).every(
        (row) => row.connectKind === "coming" && !connectorConnectsInApp(row),
      ),
    ).toBe(true);
  });
});
