export type SettingsSectionId =
  | "general"
  | "appearance"
  | "account"
  | "archived"
  | "extensions"
  | "runtime"
  | "about";

export type SettingsSectionGroup = "personal" | "system";

export type SettingsSectionIcon =
  | "settings"
  | "appearance"
  | "user"
  | "archive"
  | "extensions"
  | "doctor"
  | "info";

export interface SettingsSectionRegistration {
  id: SettingsSectionId;
  icon: SettingsSectionIcon;
  labelKey: string;
  group: SettingsSectionGroup;
  /** Existing, searchable settings contained by this section. */
  keywordKeys: readonly string[];
}

/**
 * Settings navigation is capability-honest: this registry contains only
 * sections backed by the current UI and Host. Future sections belong here
 * only after their data and commands exist.
 */
export const SETTINGS_REGISTRY: readonly SettingsSectionRegistration[] = [
  {
    id: "general",
    icon: "settings",
    labelKey: "settings.nav.general",
    group: "personal",
    keywordKeys: [
      "settings.section.composer",
      "settings.section.permissions",
      "settings.section.general",
      "settings.prefsScope",
      "settings.availableModels",
      "settings.permissionDeep",
      "settings.language",
      "settings.sessionDataMode",
      "settings.openTarget",
      "settings.storeApiKeysInKeychain",
    ],
  },
  {
    id: "appearance",
    icon: "appearance",
    labelKey: "settings.nav.appearance",
    group: "personal",
    keywordKeys: [
      "settings.theme",
      "settings.themeLight",
      "settings.themeDark",
      "settings.themeHighContrast",
    ],
  },
  {
    id: "account",
    icon: "user",
    labelKey: "settings.nav.account",
    group: "personal",
    keywordKeys: [
      "account.profiles",
      "account.usageSummary",
      "account.login",
      "account.logout",
      "providers.title",
    ],
  },
  {
    id: "archived",
    icon: "archive",
    labelKey: "settings.nav.archived",
    group: "personal",
    keywordKeys: [
      "settings.archived.desc",
      "settings.archived.restore",
      "settings.archived.delete",
    ],
  },
  {
    id: "extensions",
    icon: "extensions",
    labelKey: "settings.nav.extensions",
    group: "system",
    keywordKeys: [
      "ext.skills.title",
      "ext.mcp.title",
      "ext.plugins.title",
      "ext.refresh",
    ],
  },
  {
    id: "runtime",
    icon: "doctor",
    labelKey: "settings.nav.runtime",
    group: "system",
    keywordKeys: [
      "settings.cliPath",
      "settings.acpServer",
      "settings.maxConcurrentAgents",
      "settings.agentIdleMinutes",
      "settings.streamStallSeconds",
      "settings.doctorDesc",
    ],
  },
  {
    id: "about",
    icon: "info",
    labelKey: "settings.nav.about",
    group: "system",
    keywordKeys: [
      "settings.aboutApp",
      "app.versionFooter",
      "doctor.title",
    ],
  },
] as const;

export function filterSettingsRegistry(
  query: string,
  translate: (key: string) => string,
): readonly SettingsSectionRegistration[] {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return SETTINGS_REGISTRY;

  return SETTINGS_REGISTRY.filter((section) =>
    [section.labelKey, ...section.keywordKeys].some((key) =>
      translate(key).toLocaleLowerCase().includes(normalized),
    ),
  );
}
