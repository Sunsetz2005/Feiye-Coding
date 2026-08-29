export type ConnectorAudience = "public" | "personal";
export type ConnectorCategory = "featured" | "productivity";
export type ConnectorStatus = "available" | "connected" | "error";
export type ConnectorConnectKind = "in_app" | "coming";

export interface ConnectorPrompt {
  id: string;
  text: string;
}

export interface ConnectorCatalogEntry {
  id: string;
  nameKey: string;
  descriptionKey: string;
  aboutKey: string;
  developer: string;
  version: string;
  capabilities: string;
  category: ConnectorCategory;
  audience: ConnectorAudience;
  website: string;
  privacy: string;
  terms: string;
  openConnectorSlug: string;
  /** GitHub PAT and Google OAuth (Gmail, Drive, Calendar) connect in-app. */
  connectKind: ConnectorConnectKind;
  color: string;
  glyph: string;
  prompts: ConnectorPrompt[];
}

export const CONNECTOR_CATALOG: readonly ConnectorCatalogEntry[] = [
  {
    id: "gmail",
    nameKey: "plugin.gmail.name",
    descriptionKey: "plugin.gmail.desc",
    aboutKey: "plugin.gmail.about",
    developer: "Google",
    version: "0.1.11",
    capabilities: "Interactive, Write",
    category: "featured",
    audience: "public",
    website: "https://mail.google.com",
    privacy: "https://policies.google.com/privacy",
    terms: "https://policies.google.com/terms",
    openConnectorSlug: "gmail",
    connectKind: "in_app",
    color: "#ea4335",
    glyph: "M",
    prompts: [
      {
        id: "gmail-1",
        text: "Summarize the last 5 messages in [subject line] and capture decisions, open questions, and what I should follow up on next",
      },
      {
        id: "gmail-2",
        text: "Draft a polite, firm reply to our auditor's latest email, with a short bullet list of exactly what we'll provide",
      },
      {
        id: "gmail-3",
        text: "Turn my latest customer escalation thread into an action tracker with owners, deadlines, and an email reference for each item",
      },
    ],
  },
  {
    id: "github",
    nameKey: "plugin.github.name",
    descriptionKey: "plugin.github.desc",
    aboutKey: "plugin.github.about",
    developer: "GitHub",
    version: "0.1.8",
    capabilities: "Interactive, Write",
    category: "featured",
    audience: "public",
    website: "https://github.com",
    privacy: "https://docs.github.com/site-policy/privacy-policies/github-privacy-statement",
    terms: "https://docs.github.com/site-policy/github-terms/github-terms-of-service",
    openConnectorSlug: "github",
    connectKind: "in_app",
    color: "#24292f",
    glyph: "GH",
    prompts: [
      { id: "gh-1", text: "Triage open pull requests in this repo and list what is blocked, waiting on review, or ready to merge" },
      { id: "gh-2", text: "Summarize issues labeled bug from the last 7 days and suggest an owner for each" },
      { id: "gh-3", text: "Draft a release note from merged PRs since the last tag" },
    ],
  },
  {
    id: "google-drive",
    nameKey: "plugin.drive.name",
    descriptionKey: "plugin.drive.desc",
    aboutKey: "plugin.drive.about",
    developer: "Google",
    version: "0.1.6",
    capabilities: "Interactive, Write",
    category: "featured",
    audience: "public",
    website: "https://drive.google.com",
    privacy: "https://policies.google.com/privacy",
    terms: "https://policies.google.com/terms",
    openConnectorSlug: "google-drive",
    connectKind: "in_app",
    color: "#0f9d58",
    glyph: "D",
    prompts: [
      { id: "drive-1", text: "Find the latest shared briefing in Drive and summarize decisions and open questions" },
      { id: "drive-2", text: "List docs I edited this week and group them by project" },
      { id: "drive-3", text: "Create a one-page outline in a new Doc from this conversation" },
    ],
  },
  {
    id: "google-calendar",
    nameKey: "plugin.calendar.name",
    descriptionKey: "plugin.calendar.desc",
    aboutKey: "plugin.calendar.about",
    developer: "Google",
    version: "0.1.6",
    capabilities: "Interactive, Write",
    category: "featured",
    audience: "public",
    website: "https://calendar.google.com",
    privacy: "https://policies.google.com/privacy",
    terms: "https://policies.google.com/terms",
    openConnectorSlug: "google-calendar",
    connectKind: "in_app",
    color: "#1967d2",
    glyph: "31",
    prompts: [
      { id: "cal-1", text: "Show my next three meetings and what I should prepare for each" },
      { id: "cal-2", text: "Find a 45-minute slot this week for a design review with the core team" },
      { id: "cal-3", text: "Turn yesterday's meetings into a follow-up checklist with owners" },
    ],
  },
  {
    id: "notion",
    nameKey: "plugin.notion.name",
    descriptionKey: "plugin.notion.desc",
    aboutKey: "plugin.notion.about",
    developer: "Notion",
    version: "0.1.4",
    capabilities: "Interactive, Write",
    category: "featured",
    audience: "public",
    website: "https://www.notion.so",
    privacy: "https://www.notion.so/privacy",
    terms: "https://www.notion.so/terms",
    openConnectorSlug: "notion",
    connectKind: "coming",
    color: "#111111",
    glyph: "N",
    prompts: [
      { id: "notion-1", text: "Find the project spec for this workspace and list decisions that still need an owner" },
      { id: "notion-2", text: "Draft a meeting note page from this conversation" },
      { id: "notion-3", text: "Update the weekly status page with what shipped, what's blocked, and next" },
    ],
  },
  {
    id: "slack",
    nameKey: "plugin.slack.name",
    descriptionKey: "plugin.slack.desc",
    aboutKey: "plugin.slack.about",
    developer: "Slack",
    version: "0.1.5",
    capabilities: "Interactive, Write",
    category: "featured",
    audience: "public",
    website: "https://slack.com",
    privacy: "https://slack.com/privacy-policy",
    terms: "https://slack.com/terms-of-service",
    openConnectorSlug: "slack",
    connectKind: "coming",
    color: "#611f69",
    glyph: "#",
    prompts: [
      { id: "slack-1", text: "Summarize unread mentions from the last day and flag anything that needs a reply" },
      { id: "slack-2", text: "Draft a concise standup update from this conversation for #eng" },
      { id: "slack-3", text: "Find the latest incident thread and extract current status, owners, and next check-in" },
    ],
  },
  {
    id: "granola",
    nameKey: "plugin.granola.name",
    descriptionKey: "plugin.granola.desc",
    aboutKey: "plugin.granola.about",
    developer: "Granola",
    version: "0.1.2",
    capabilities: "Interactive",
    category: "productivity",
    audience: "public",
    website: "https://www.granola.ai",
    privacy: "https://www.granola.ai/privacy",
    terms: "https://www.granola.ai/terms",
    openConnectorSlug: "granola",
    connectKind: "coming",
    color: "#1f7a4d",
    glyph: "G",
    prompts: [
      { id: "granola-1", text: "Add this conversation as meeting context for my next 1:1" },
      { id: "granola-2", text: "Pull action items from yesterday's notes and assign owners" },
      { id: "granola-3", text: "Write a recap of the last product review using Granola notes" },
    ],
  },
  {
    id: "fireflies",
    nameKey: "plugin.fireflies.name",
    descriptionKey: "plugin.fireflies.desc",
    aboutKey: "plugin.fireflies.about",
    developer: "Fireflies",
    version: "0.1.2",
    capabilities: "Interactive",
    category: "productivity",
    audience: "public",
    website: "https://fireflies.ai",
    privacy: "https://fireflies.ai/privacy",
    terms: "https://fireflies.ai/terms",
    openConnectorSlug: "fireflies",
    connectKind: "coming",
    color: "#ec4899",
    glyph: "F",
    prompts: [
      { id: "ff-1", text: "Search meeting transcripts for decisions about the launch date" },
      { id: "ff-2", text: "Summarize the last customer call and list objections" },
      { id: "ff-3", text: "Extract quotes I can use in a follow-up email" },
    ],
  },
  {
    id: "outlook",
    nameKey: "plugin.outlook.name",
    descriptionKey: "plugin.outlook.desc",
    aboutKey: "plugin.outlook.about",
    developer: "Microsoft",
    version: "0.1.3",
    capabilities: "Interactive, Write",
    category: "productivity",
    audience: "public",
    website: "https://outlook.live.com",
    privacy: "https://privacy.microsoft.com",
    terms: "https://www.microsoft.com/servicesagreement",
    openConnectorSlug: "outlook",
    connectKind: "coming",
    color: "#0078d4",
    glyph: "O",
    prompts: [
      { id: "out-1", text: "Summarize unread Outlook mail from today by sender" },
      { id: "out-2", text: "Draft a reply to the latest calendar invitation asking to move it 30 minutes later" },
      { id: "out-3", text: "Find the last email about invoices and list outstanding amounts" },
    ],
  },
  {
    id: "plaud",
    nameKey: "plugin.plaud.name",
    descriptionKey: "plugin.plaud.desc",
    aboutKey: "plugin.plaud.about",
    developer: "Plaud",
    version: "0.1.1",
    capabilities: "Interactive",
    category: "productivity",
    audience: "public",
    website: "https://www.plaud.ai",
    privacy: "https://www.plaud.ai/privacy",
    terms: "https://www.plaud.ai/terms",
    openConnectorSlug: "plaud",
    connectKind: "coming",
    color: "#111111",
    glyph: "P",
    prompts: [
      { id: "plaud-1", text: "Turn my latest recording into a structured meeting note" },
      { id: "plaud-2", text: "List action items from the last two recordings" },
      { id: "plaud-3", text: "Find mentions of budget in recent transcripts" },
    ],
  },
];

export function connectorById(id: string): ConnectorCatalogEntry | undefined {
  return CONNECTOR_CATALOG.find((entry) => entry.id === id);
}

export function featuredConnectors(): ConnectorCatalogEntry[] {
  return CONNECTOR_CATALOG.filter((entry) => entry.category === "featured");
}

export function productivityConnectors(): ConnectorCatalogEntry[] {
  return CONNECTOR_CATALOG.filter((entry) => entry.category === "productivity");
}

export function connectorConnectsInApp(entry: ConnectorCatalogEntry): boolean {
  return entry.connectKind === "in_app";
}

export function connectorUsesGoogleSignIn(entry: ConnectorCatalogEntry): boolean {
  return (
    entry.id === "gmail" ||
    entry.id === "google-drive" ||
    entry.id === "google-calendar"
  );
}
