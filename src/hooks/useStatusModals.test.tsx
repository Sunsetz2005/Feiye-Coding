// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { inspectMcp } from "@/lib/api";
import { useStatusModals } from "./useStatusModals";

vi.mock("@/lib/api", () => ({
  inspectMcp: vi.fn(),
}));

const inspectMcpMock = vi.mocked(inspectMcp);

beforeEach(() => {
  inspectMcpMock.mockReset();
});

describe("useStatusModals", () => {
  it("opens and closes the status modal", () => {
    const { result } = renderHook(() => useStatusModals());

    act(() => result.current.openStatusModal());
    expect(result.current.showStatusModal).toBe(true);

    act(() => result.current.closeStatusModal());
    expect(result.current.showStatusModal).toBe(false);
  });

  it("opens the MCP modal, loads the project servers, and clears loading", async () => {
    let resolveInspect!: (value: {
      servers: Array<{ name: string }>;
    }) => void;
    inspectMcpMock
      .mockResolvedValueOnce({ servers: [], error: "stale error" })
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveInspect = resolve;
        }),
      );
    const { result } = renderHook(() => useStatusModals());

    await act(async () => {
      await result.current.openMcpModal("/workspace/old-project");
    });
    expect(result.current.mcpError).toBe("stale error");

    let opening!: Promise<void>;
    act(() => {
      opening = result.current.openMcpModal("/workspace/project");
    });

    expect(result.current.showMcpModal).toBe(true);
    expect(result.current.mcpLoading).toBe(true);
    expect(result.current.mcpError).toBeNull();
    expect(inspectMcpMock).toHaveBeenLastCalledWith("/workspace/project");

    await act(async () => {
      resolveInspect({ servers: [{ name: "filesystem" }] });
      await opening;
    });

    expect(result.current.mcpServers).toEqual([{ name: "filesystem" }]);
    expect(result.current.mcpLoading).toBe(false);
  });

  it("surfaces a Host-returned MCP error alongside discovered servers", async () => {
    inspectMcpMock.mockResolvedValue({
      servers: [{ name: "github" }],
      error: "inspect unavailable",
    });
    const { result } = renderHook(() => useStatusModals());

    await act(async () => {
      await result.current.openMcpModal(null);
    });

    expect(result.current.mcpServers).toEqual([{ name: "github" }]);
    expect(result.current.mcpError).toBe("inspect unavailable");
    expect(result.current.mcpLoading).toBe(false);
  });

  it("clears servers and stringifies an inspect exception", async () => {
    inspectMcpMock
      .mockResolvedValueOnce({ servers: [{ name: "stale" }] })
      .mockRejectedValueOnce(new Error("inspect failed"));
    const { result } = renderHook(() => useStatusModals());

    await act(async () => {
      await result.current.openMcpModal("/workspace/project");
      await result.current.openMcpModal("/workspace/project");
    });

    expect(result.current.mcpServers).toEqual([]);
    expect(result.current.mcpError).toBe("Error: inspect failed");
    expect(result.current.mcpLoading).toBe(false);
  });

  it("only hides the MCP modal when closing it", async () => {
    inspectMcpMock.mockResolvedValue({
      servers: [{ name: "filesystem" }],
      error: "partial result",
    });
    const { result } = renderHook(() => useStatusModals());

    await act(async () => {
      await result.current.openMcpModal("/workspace/project");
    });
    act(() => result.current.closeMcpModal());

    expect(result.current.showMcpModal).toBe(false);
    expect(result.current.mcpServers).toEqual([{ name: "filesystem" }]);
    expect(result.current.mcpError).toBe("partial result");
  });
});
