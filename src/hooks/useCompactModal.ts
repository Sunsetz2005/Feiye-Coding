import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
} from "react";

export interface UseCompactModalApi {
  showCompactModal: boolean;
  compactNote: string;
  setCompactNote: Dispatch<SetStateAction<string>>;
  compactNoteRef: RefObject<HTMLInputElement | null>;
  openCompactModal: () => void;
  closeCompactModal: () => void;
}

export function useCompactModal(): UseCompactModalApi {
  const [showCompactModal, setShowCompactModal] = useState(false);
  const [compactNote, setCompactNote] = useState("");
  const compactNoteRef = useRef<HTMLInputElement>(null);

  const openCompactModal = useCallback(() => {
    setCompactNote("");
    setShowCompactModal(true);
  }, []);

  const closeCompactModal = useCallback(() => {
    setShowCompactModal(false);
    setCompactNote("");
  }, []);

  useEffect(() => {
    if (!showCompactModal) return;
    const timer = window.setTimeout(() => {
      compactNoteRef.current?.focus();
    }, 0);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeCompactModal();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("keydown", onKey);
    };
  }, [closeCompactModal, showCompactModal]);

  return {
    showCompactModal,
    compactNote,
    setCompactNote,
    compactNoteRef,
    openCompactModal,
    closeCompactModal,
  };
}
