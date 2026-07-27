import {
  createContext,
  useCallback,
  useContext,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  type ReactNode,
} from "react";

export type FloatingSurfaceCloseReason = "ownership-handoff";
export type FloatingSurfaceClose = (
  reason: FloatingSurfaceCloseReason,
) => void;

type FloatingSurfaceEntry = {
  id: string;
  close: FloatingSurfaceClose;
};

/**
 * Small imperative core kept separate from React so ownership ordering is
 * deterministic and independently testable.
 *
 * A new owner is installed before the previous close callback runs. This is
 * important: the previous surface may release itself while closing, and that
 * stale release must never clear the new owner.
 */
export function createFloatingSurfaceOwner() {
  let current: FloatingSurfaceEntry | null = null;

  return {
    claim(id: string, close: FloatingSurfaceClose) {
      const previous = current;
      if (previous?.id === id) {
        current = { id, close };
        return;
      }

      current = { id, close };
      previous?.close("ownership-handoff");
    },

    release(id: string) {
      if (current?.id === id) current = null;
    },

    get activeId() {
      return current?.id ?? null;
    },
  };
}

type FloatingSurfaceContextValue = {
  claim: (id: string, close: FloatingSurfaceClose) => void;
  release: (id: string) => void;
};

const FloatingSurfaceContext =
  createContext<FloatingSurfaceContextValue | null>(null);

/**
 * Owns the single floating surface for the workbench.
 *
 * The value is deliberately stable and carries no reactive `activeId`, so
 * opening a menu does not re-render the entire App tree.
 */
export function FloatingSurfaceProvider({
  children,
}: {
  children: ReactNode;
}) {
  const ownerRef = useRef<ReturnType<typeof createFloatingSurfaceOwner> | null>(
    null,
  );
  if (!ownerRef.current) ownerRef.current = createFloatingSurfaceOwner();

  const claim = useCallback((id: string, close: FloatingSurfaceClose) => {
    ownerRef.current?.claim(id, close);
  }, []);
  const release = useCallback((id: string) => {
    ownerRef.current?.release(id);
  }, []);
  const value = useMemo(() => ({ claim, release }), [claim, release]);

  return (
    <FloatingSurfaceContext.Provider value={value}>
      {children}
    </FloatingSurfaceContext.Provider>
  );
}

/**
 * Registers an already-controlled popover with the workbench owner.
 *
 * Ownership is claimed in a layout effect, before paint. Handoff only asks the
 * prior component to run its existing close path; outside-click, Escape,
 * animation, and focus behavior therefore remain component-owned.
 */
export function useFloatingSurfacePresence({
  open,
  onRequestClose,
  id,
}: {
  open: boolean;
  onRequestClose: () => void;
  id?: string;
}) {
  const generatedId = useId();
  const surfaceId = id ?? `floating-surface:${generatedId}`;
  const owner = useContext(FloatingSurfaceContext);
  const closeRef = useRef(onRequestClose);
  closeRef.current = onRequestClose;

  const closeForHandoff = useCallback<FloatingSurfaceClose>(() => {
    closeRef.current();
  }, []);

  const claim = owner?.claim;
  const release = owner?.release;

  useLayoutEffect(() => {
    if (!open || !claim || !release) return;
    claim(surfaceId, closeForHandoff);
    return () => release(surfaceId);
  }, [claim, closeForHandoff, open, release, surfaceId]);
}
