import { Banner } from "@/shared/ui";

export interface BackgroundApprovalBannerLabels {
  one: string;
  many: (n: number) => string;
  go: string;
}

export interface BackgroundApprovalBannerProps {
  /** Sessions other than the focused one with a pending permission, ask-user,
   * or plan interaction (`pendingAskSessionIds` — same set the sidebar's `?`
   * badge already reads from `session://interaction`). */
  pendingSessionIds: ReadonlySet<string>;
  labels: BackgroundApprovalBannerLabels;
  /** Switch the focused session — reuses the app's existing session-open path. */
  onGo: (sessionId: string) => void;
}

/**
 * Top-of-conversation banner surfacing background sessions waiting on a
 * decision (permission, ask-user, or plan). Not a notification system of its
 * own — it renders the same `pendingAskSessionIds` set the sidebar already
 * tracks, and jumping to a session clears its entry from that set.
 */
export function BackgroundApprovalBanner({
  pendingSessionIds,
  labels,
  onGo,
}: BackgroundApprovalBannerProps) {
  if (pendingSessionIds.size === 0) return null;
  const [firstPendingId] = pendingSessionIds;
  return (
    <Banner
      tone="warning"
      action={{
        label: labels.go,
        onPress: () => {
          if (firstPendingId) onGo(firstPendingId);
        },
      }}
    >
      {pendingSessionIds.size === 1
        ? labels.one
        : labels.many(pendingSessionIds.size)}
    </Banner>
  );
}
