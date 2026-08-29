import { describe, expect, it } from "vitest";
import {
  filterSettingsRegistry,
  SETTINGS_REGISTRY,
} from "@/lib/settingsRegistry";

const copy: Record<string, string> = {
  "settings.nav.general": "General",
  "settings.nav.appearance": "Appearance",
  "settings.nav.account": "Profile",
  "settings.nav.models": "My models",
  "settings.nav.archived": "Archived",
  "settings.nav.extensions": "Extensions",
  "settings.nav.runtime": "Runtime",
  "settings.nav.about": "About",
  "settings.runtime.kernel": "Built-in Sunsetz Runtime",
  "settings.runtime.lead":
    "Sunsetz Runtime is built into this app",
  "settings.permissionDeep": "Default access policy",
  "settings.theme.dark": "Dark theme",
  "account.usage": "Usage",
  "ext.mcp": "MCP servers",
  "settings.cliPath": "Runtime executable",
  "settings.license": "License",
};

const t = (key: string) => copy[key] ?? key;

describe("settings registry", () => {
  it("contains only the currently implemented sections", () => {
    expect(SETTINGS_REGISTRY.map((section) => section.id)).toEqual([
      "general",
      "appearance",
      "account",
      "models",
      "archived",
      "extensions",
      "runtime",
      "about",
    ]);
  });

  it("finds a section by a concrete setting, not only its title", () => {
    expect(filterSettingsRegistry("access policy", t).map((item) => item.id))
      .toEqual(["general"]);
    expect(filterSettingsRegistry("MCP", t).map((item) => item.id))
      .toEqual(["extensions"]);
    expect(filterSettingsRegistry("executable", t).map((item) => item.id))
      .toEqual(["runtime"]);
    expect(filterSettingsRegistry("built-in", t).map((item) => item.id))
      .toEqual(["runtime"]);
  });

  it("returns the full registry for an empty query", () => {
    expect(filterSettingsRegistry("  ", t)).toBe(SETTINGS_REGISTRY);
  });
});
