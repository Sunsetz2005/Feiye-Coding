// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProvidersPanel, uniqueProviderId } from "./ProvidersPanel";
import * as api from "@/lib/api";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const emptyList: api.ProvidersListResult = {
  providers: [],
  defaultModel: null,
  activeSource: "official",
  activeProviderId: null,
  configPath: "",
  agentHome: "",
};

describe("uniqueProviderId", () => {
  it("slugs the display name and uniquifies collisions", () => {
    expect(uniqueProviderId("Work Relay", [])).toBe("work-relay");
    expect(uniqueProviderId("Work Relay", ["work-relay"])).toBe("work-relay-2");
    expect(uniqueProviderId("Work Relay", ["work-relay", "work-relay-2"])).toBe(
      "work-relay-3",
    );
    expect(uniqueProviderId("   ", [])).toBe("provider");
    expect(uniqueProviderId("", ["provider"])).toBe("provider-2");
  });
});

describe("ProvidersPanel", () => {
  it("hides config id and only shows display fields", async () => {
    render(<ProvidersPanel locale="en" />);
    fireEvent.click(await screen.findByRole("button", { name: "Add provider" }));
    expect(screen.getByText("Display name")).toBeTruthy();
    expect(screen.getByText("Base URL")).toBeTruthy();
    expect(screen.getByText("Message format")).toBeTruthy();
    expect(screen.getByText("API key")).toBeTruthy();
    expect(screen.getByText("Request model")).toBeTruthy();
    expect(screen.queryByText("Config id")).toBeNull();
  });

  it("saves a slug generated from the display name", async () => {
    vi.spyOn(api, "isTauri").mockReturnValue(true);
    vi.spyOn(api, "providersList").mockResolvedValue(emptyList);
    vi.spyOn(api, "providersPing").mockResolvedValue({
      ok: true,
      latencyMs: 12,
      endpoint: "https://api.example.com/v1/models",
      status: 200,
    });
    const upsert = vi.spyOn(api, "providersUpsert").mockResolvedValue({
      ...emptyList,
      providers: [
        {
          id: "work-relay",
          model: "gpt-4.1",
          baseUrl: "https://api.example.com/v1",
          name: "Work Relay",
          hasApiKey: true,
          apiBackend: "responses",
          isDefault: true,
        },
      ],
      defaultModel: "work-relay",
      activeSource: "custom",
      activeProviderId: "work-relay",
    });

    render(<ProvidersPanel locale="en" />);
    fireEvent.click(await screen.findByRole("button", { name: "Add provider" }));
    fireEvent.change(screen.getByPlaceholderText("e.g. Work relay"), {
      target: { value: "Work Relay" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("https://your-relay.example.com/v1"),
      { target: { value: "https://api.example.com/v1" } },
    );
    fireEvent.change(screen.getByPlaceholderText("sk-…"), {
      target: { value: "sk-test" },
    });
    fireEvent.change(screen.getByPlaceholderText("e.g. model-id"), {
      target: { value: "gpt-4.1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => {
      expect(upsert).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "work-relay",
          name: "Work Relay",
          baseUrl: "https://api.example.com/v1",
          model: "gpt-4.1",
          createOnly: true,
        }),
      );
    });
  });

  it("opens the fetched model list without auto-filling the first id", async () => {
    vi.spyOn(api, "isTauri").mockReturnValue(true);
    vi.spyOn(api, "providersList").mockResolvedValue(emptyList);
    vi.spyOn(api, "providersListModels").mockResolvedValue({
      endpoint: "https://api.example.com/v1/models",
      models: [{ id: "claude-opus-4-6" }, { id: "claude-haiku-4-5" }],
    });

    render(<ProvidersPanel locale="en" />);
    fireEvent.click(await screen.findByRole("button", { name: "Add provider" }));
    fireEvent.change(
      screen.getByPlaceholderText("https://your-relay.example.com/v1"),
      { target: { value: "https://api.example.com/v1" } },
    );
    fireEvent.change(screen.getByPlaceholderText("sk-…"), {
      target: { value: "sk-test" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Fetch models" }));

    expect(await screen.findByRole("option", { name: "claude-opus-4-6" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "claude-haiku-4-5" })).toBeTruthy();
    expect(
      (screen.getByPlaceholderText("e.g. model-id") as HTMLInputElement).value,
    ).toBe("");

    fireEvent.click(screen.getByRole("option", { name: "claude-haiku-4-5" }));
    expect(
      (screen.getByPlaceholderText("e.g. model-id") as HTMLInputElement).value,
    ).toBe("claude-haiku-4-5");
  });
});
