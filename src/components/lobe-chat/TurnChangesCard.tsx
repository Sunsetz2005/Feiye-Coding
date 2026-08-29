import { useMemo, useState } from "react";
import { IconFileDiff } from "@/components/icons";
import {
  countLineEdits,
  pathRelativeToProject,
  type SessionFileChange,
} from "@/lib/sessionChanges";

const PREVIEW_LIMIT = 3;

export function TurnChangesCard({
  changes,
  projectPath,
  labels,
  onReview,
  onOpenFile,
}: {
  changes: SessionFileChange[];
  projectPath?: string | null;
  labels: {
    title: string;
    review: string;
    showMore: string;
    showLess: string;
  };
  onReview: () => void;
  onOpenFile: (change: SessionFileChange) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const totals = useMemo(() => {
    let added = 0;
    let removed = 0;
    let known = false;
    for (const change of changes) {
      const stats = countLineEdits(change.before, change.after);
      if (!stats) continue;
      known = true;
      added += stats.added;
      removed += stats.removed;
    }
    return known ? { added, removed } : null;
  }, [changes]);
  const visible = expanded ? changes : changes.slice(0, PREVIEW_LIMIT);
  const hidden = Math.max(0, changes.length - PREVIEW_LIMIT);

  if (changes.length === 0) return null;

  return (
    <div className="turn-changes" data-testid="turn-changes">
      <div className="turn-changes__head">
        <span className="turn-changes__icon" aria-hidden>
          <IconFileDiff size={16} />
        </span>
        <div className="turn-changes__title-wrap">
          <div className="turn-changes__title">{labels.title}</div>
          {totals ? (
            <div className="turn-changes__stats">
              <span className="turn-changes__add">+{totals.added}</span>
              <span className="turn-changes__del">-{totals.removed}</span>
            </div>
          ) : null}
        </div>
        <button
          type="button"
          className="turn-changes__review"
          onClick={onReview}
        >
          {labels.review}
        </button>
      </div>
      <ul className="turn-changes__list">
        {visible.map((change) => {
          const rel = pathRelativeToProject(change.path, projectPath);
          const stats = countLineEdits(change.before, change.after);
          return (
            <li key={change.path}>
              <button
                type="button"
                className="turn-changes__file"
                onClick={() => onOpenFile(change)}
              >
                <span className="turn-changes__path" title={change.path}>
                  {rel || change.name}
                </span>
                {stats ? (
                  <span className="turn-changes__file-stats">
                    <span className="turn-changes__add">+{stats.added}</span>
                    <span className="turn-changes__del">-{stats.removed}</span>
                  </span>
                ) : null}
              </button>
            </li>
          );
        })}
      </ul>
      {hidden > 0 ? (
        <button
          type="button"
          className="turn-changes__more"
          onClick={() => setExpanded((open) => !open)}
        >
          {expanded ? labels.showLess : labels.showMore}
        </button>
      ) : null}
    </div>
  );
}
