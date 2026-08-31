/** Destinations for the session "Move to project" cascade. */

export type SessionMoveDestination = {
  projectId: string | null;
  current: boolean;
};

export function sessionMoveDestinations(
  projectIds: readonly string[],
  currentProjectId: string | null,
): SessionMoveDestination[] {
  const dests: SessionMoveDestination[] = projectIds.map((id) => ({
    projectId: id,
    current: id === currentProjectId,
  }));
  dests.push({
    projectId: null,
    current: currentProjectId == null,
  });
  return dests;
}

export function sessionMoveAvailable(
  dests: readonly SessionMoveDestination[],
): boolean {
  return dests.some((item) => !item.current);
}
