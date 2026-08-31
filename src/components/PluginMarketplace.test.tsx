// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PluginMarketplace } from "./PluginMarketplace";
import { CONNECTOR_CATALOG } from "@/lib/connectorCatalog";
import { ConnectorLogo, connectorLogoSrc } from "./ConnectorLogo";
import * as api from "@/lib/api";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("PluginMarketplace", () => {
  it("renders the featured catalog with connector names and connect actions", () => {
    const html = renderToStaticMarkup(
      <PluginMarketplace locale="en" onUsePrompt={() => undefined} />,
    );
    expect(html).toContain("Plugins");
    expect(html).toContain("Gmail");
    expect(html).toContain("GitHub");
    expect(html).toContain("Connect");
    expect(html).toContain("Available now");
    expect(html).toContain("Coming soon in-app");
    expect(html).not.toContain("Open Connector");
    expect(html).toContain('data-testid="plugin-marketplace"');
  });

  it("bundles an official logo for every catalog entry", () => {
    const srcs = CONNECTOR_CATALOG.map((entry) => connectorLogoSrc(entry.id));
    expect(srcs.every(Boolean)).toBe(true);
    expect(new Set(srcs).size).toBe(CONNECTOR_CATALOG.length);
    expect(CONNECTOR_CATALOG.every((entry) => entry.prompts.length === 3)).toBe(
      true,
    );
  });

  it("opens a detail page and copies a recommended prompt", () => {
    const onUsePrompt = vi.fn();
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn(async () => undefined) },
    });
    render(<PluginMarketplace locale="en" onUsePrompt={onUsePrompt} />);
    fireEvent.click(screen.getAllByText("Gmail")[0]!);
    expect(screen.getByText(/Read and manage Gmail/i)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Connect" })).toBeTruthy();
    expect(screen.queryByText("Coming soon in-app")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Summarize the last 5/i }));
    expect(onUsePrompt).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
  });

  it("shows trusted skills and a fail-closed connect error", () => {
    render(
      <PluginMarketplace
        locale="en"
        skills={[{ id: "s1", name: "Review", description: "Review diffs" }]}
        onUsePrompt={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Skills" }));
    expect(screen.getByText("Review diffs")).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Plugins" }));
    expect(screen.getAllByRole("button", { name: "Connect" })).toHaveLength(6);
    const githubCard = screen.getAllByText("GitHub")[0]!.closest("article")!;
    fireEvent.click(within(githubCard).getByRole("button", { name: "Connect" }));
    expect(screen.getByPlaceholderText(/github_pat_/i)).toBeTruthy();
  });

  it("shows an empty personal audience", () => {
    render(<PluginMarketplace locale="en" onUsePrompt={() => undefined} />);
    fireEvent.click(screen.getByRole("tab", { name: "Personal" }));
    expect(screen.getByRole("status")).toBeTruthy();
  });

  it("opens a credential dialog when GitHub has no token", async () => {
    vi.spyOn(api, "isTauri").mockReturnValue(true);
    vi.spyOn(api, "connectorsList").mockResolvedValue([]);
    const connect = vi.spyOn(api, "connectorsConnect").mockResolvedValue({
      id: "github",
      slug: "github",
      developer: "GitHub",
      version: "0.1.8",
      enabled: true,
      connected: true,
      lastError: null,
      tools: ["github_list_pull_requests"],
    });
    render(<PluginMarketplace locale="en" onUsePrompt={() => undefined} />);
    fireEvent.click(screen.getAllByText("GitHub")[0]!);
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(await screen.findByPlaceholderText(/github_pat_/i)).toBeTruthy();
    expect(connect).not.toHaveBeenCalled();
    fireEvent.change(screen.getByPlaceholderText(/github_pat_/i), {
      target: { value: "ghp_test" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save and connect" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith("github", "ghp_test");
    });
  });

  it("starts Gmail connect in-app without a token paste dialog", async () => {
    vi.spyOn(api, "isTauri").mockReturnValue(true);
    vi.spyOn(api, "connectorsList").mockResolvedValue([]);
    const connect = vi.spyOn(api, "connectorsConnect").mockResolvedValue({
      id: "gmail",
      slug: "gmail",
      developer: "Google",
      version: "0.1.11",
      enabled: true,
      connected: true,
      lastError: null,
      tools: ["gmail_list_messages"],
    });
    render(<PluginMarketplace locale="en" onUsePrompt={() => undefined} />);
    fireEvent.click(screen.getAllByText("Gmail")[0]!);
    expect(
      screen.getByText(/Connect opens your browser so you can sign in with Google/i),
    ).toBeTruthy();
    expect(screen.queryByText(/SUNSETZ_GOOGLE_OAUTH_CLIENT_ID/)).toBeNull();
    expect(screen.queryByText(/环境变量/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(screen.queryByPlaceholderText(/github_pat_/i)).toBeNull();
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith("gmail", undefined);
    });
  });

  it("starts Drive and Calendar connect in-app without a token paste dialog", async () => {
    vi.spyOn(api, "isTauri").mockReturnValue(true);
    vi.spyOn(api, "connectorsList").mockResolvedValue([]);
    const connect = vi.spyOn(api, "connectorsConnect").mockImplementation(async (id) => ({
      id,
      slug: id,
      developer: "Google",
      version: "0.1.6",
      enabled: true,
      connected: true,
      lastError: null,
      tools: [],
    }));
    render(<PluginMarketplace locale="en" onUsePrompt={() => undefined} />);
    fireEvent.click(screen.getAllByText("Google Drive")[0]!);
    expect(
      screen.getByText(/Connect opens your browser so you can sign in with Google/i),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(screen.queryByPlaceholderText(/github_pat_/i)).toBeNull();
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith("google-drive", undefined);
    });
    fireEvent.click(screen.getByRole("button", { name: /Plugins/i }));
    fireEvent.click(screen.getAllByText("Google Calendar")[0]!);
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith("google-calendar", undefined);
    });
  });

  it("opens a credential dialog when Notion has no token", async () => {
    vi.spyOn(api, "isTauri").mockReturnValue(true);
    vi.spyOn(api, "connectorsList").mockResolvedValue([]);
    const connect = vi.spyOn(api, "connectorsConnect").mockResolvedValue({
      id: "notion",
      slug: "notion",
      developer: "Notion",
      version: "0.1.4",
      enabled: true,
      connected: true,
      lastError: null,
      tools: ["notion_search"],
    });
    render(<PluginMarketplace locale="en" onUsePrompt={() => undefined} />);
    fireEvent.click(screen.getAllByText("Notion")[0]!);
    expect(
      screen.getByText(/Connect asks for an internal integration token/i),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(await screen.findByPlaceholderText(/secret_/i)).toBeTruthy();
    expect(connect).not.toHaveBeenCalled();
    fireEvent.change(screen.getByPlaceholderText(/secret_/i), {
      target: { value: "secret_test" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save and connect" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith("notion", "secret_test");
    });
  });

  it("opens a credential dialog when Slack has no token", async () => {
    vi.spyOn(api, "isTauri").mockReturnValue(true);
    vi.spyOn(api, "connectorsList").mockResolvedValue([]);
    const connect = vi.spyOn(api, "connectorsConnect").mockResolvedValue({
      id: "slack",
      slug: "slack",
      developer: "Slack",
      version: "0.1.5",
      enabled: true,
      connected: true,
      lastError: null,
      tools: ["slack_list_conversations"],
    });
    render(<PluginMarketplace locale="en" onUsePrompt={() => undefined} />);
    fireEvent.click(screen.getAllByText("Slack")[0]!);
    expect(screen.getByText(/Connect asks for a bot token/i)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(await screen.findByPlaceholderText(/xoxb-/i)).toBeTruthy();
    expect(connect).not.toHaveBeenCalled();
    fireEvent.change(screen.getByPlaceholderText(/xoxb-/i), {
      target: { value: "xoxb-test" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save and connect" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith("slack", "xoxb-test");
    });
  });

  it("renders the official connector mark", () => {
    const { container, rerender } = render(
      <ConnectorLogo
        connector={CONNECTOR_CATALOG[0]!}
        size={56}
      />,
    );
    expect(container.querySelector(".connector-logo--gmail")).toBeTruthy();
    expect(container.querySelector("img.connector-logo__img")).toBeTruthy();
    rerender(<ConnectorLogo connector={CONNECTOR_CATALOG[0]!} />);
    expect(container.querySelector(".connector-logo--gmail img")).toBeTruthy();
  });
});

