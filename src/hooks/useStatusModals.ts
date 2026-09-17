import { useCallback, useState } from "react";
import { inspectMcp, type McpDto } from "@/lib/api";

export interface UseStatusModalsApi {
  showStatusModal: boolean;
  showMcpModal: boolean;
  mcpServers: McpDto[];
  mcpError: string | null;
  mcpLoading: boolean;
  openStatusModal: () => void;
  closeStatusModal: () => void;
  openMcpModal: (projectPath?: string | null) => Promise<void>;
  closeMcpModal: () => void;
}

export function useStatusModals(): UseStatusModalsApi {
  const [showStatusModal, setShowStatusModal] = useState(false);
  const [showMcpModal, setShowMcpModal] = useState(false);
  const [mcpServers, setMcpServers] = useState<McpDto[]>([]);
  const [mcpError, setMcpError] = useState<string | null>(null);
  const [mcpLoading, setMcpLoading] = useState(false);

  const openStatusModal = useCallback(() => setShowStatusModal(true), []);
  const closeStatusModal = useCallback(() => setShowStatusModal(false), []);
  const closeMcpModal = useCallback(() => setShowMcpModal(false), []);

  const openMcpModal = useCallback(async (projectPath?: string | null) => {
    setShowMcpModal(true);
    setMcpLoading(true);
    setMcpError(null);
    try {
      const result = await inspectMcp(projectPath);
      setMcpServers(result.servers ?? []);
      if (result.error) setMcpError(result.error);
    } catch (error) {
      setMcpServers([]);
      setMcpError(String(error));
    } finally {
      setMcpLoading(false);
    }
  }, []);

  return {
    showStatusModal,
    showMcpModal,
    mcpServers,
    mcpError,
    mcpLoading,
    openStatusModal,
    closeStatusModal,
    openMcpModal,
    closeMcpModal,
  };
}
