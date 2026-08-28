import type { Locale } from "@/i18n";
import type {
  MemoryCandidateTypeV1,
  MemoryContextPackV1,
} from "@/lib/api";

const MEMORY_CONTENT_PREVIEW_CHARS = 160;

export type MemoryContextBadgeLabels = {
  regionLabel: string;
  title: string;
  reviewedContext: string;
  notInstructions: string;
  noFtsSessionEvidence: string;
  itemCount: string;
  clear: string;
  content: string;
  expandItem: string;
  fullContent: string;
  emptyContent: string;
  provenance: string;
  typeLabels: Record<MemoryCandidateTypeV1, string>;
};

export type MemoryContextBadgeProps = {
  pack: MemoryContextPackV1 | null;
  locale: Locale;
  labels: MemoryContextBadgeLabels;
  disabled?: boolean;
  onClear: () => void;
};

function formatLabel(
  template: string,
  values: Record<string, string | number>,
) {
  return template.replace(/\{(\w+)\}/g, (match, key: string) => {
    const value = values[key];
    return value == null ? match : String(value);
  });
}

function boundedPreview(content: string) {
  const characters = Array.from(content);
  if (characters.length <= MEMORY_CONTENT_PREVIEW_CHARS) {
    return { preview: content, truncated: false };
  }
  return {
    preview: `${characters.slice(0, MEMORY_CONTENT_PREVIEW_CHARS - 1).join("")}…`,
    truncated: true,
  };
}

function localeLanguage(locale: Locale) {
  if (locale === "zh") return "zh-CN";
  return locale;
}

/**
 * Read-only disclosure for an explicit, user-reviewed Memory context pack.
 * Runtime injection and state ownership stay with the parent; this component
 * only renders the exact reviewed values and delegates removal.
 */
export function MemoryContextBadge({
  pack,
  locale,
  labels,
  disabled = false,
  onClear,
}: MemoryContextBadgeProps) {
  if (!pack) return null;

  return (
    <section
      className="memory-context-badge"
      aria-label={labels.regionLabel}
      data-testid="memory-context-badge"
      lang={localeLanguage(locale)}
    >
      <header className="memory-context-badge__header">
        <div className="memory-context-badge__heading">
          <strong className="memory-context-badge__title">{labels.title}</strong>
          <span className="memory-context-badge__count">
            {formatLabel(labels.itemCount, { count: pack.items.length })}
          </span>
        </div>
        <button
          type="button"
          className="btn btn--ghost btn--sm memory-context-badge__clear"
          disabled={disabled}
          onClick={onClear}
        >
          {labels.clear}
        </button>
      </header>

      <div className="memory-context-badge__boundary" role="note">
        <span>{labels.reviewedContext}</span>{" "}
        <span>{labels.notInstructions}</span>{" "}
        <span>{labels.noFtsSessionEvidence}</span>
      </div>

      <ol className="memory-context-badge__items">
        {pack.items.map((item, index) => {
          const typeLabel = labels.typeLabels[item.type];
          const { preview, truncated } = boundedPreview(item.content);
          const itemNumber = index + 1;

          return (
            <li
              className="memory-context-badge__item"
              key={`${item.candidateId}:${item.contentHash}:${index}`}
            >
              <div className="memory-context-badge__item-meta">
                <span className="memory-context-badge__type">{typeLabel}</span>
                <span className="memory-context-badge__provenance">
                  {formatLabel(labels.provenance, {
                    sessionId: item.source.sessionId,
                    messageId: item.source.messageId,
                  })}
                </span>
              </div>

              <div className="memory-context-badge__content">
                <span className="memory-context-badge__content-label">
                  {labels.content}
                </span>
                {truncated ? (
                  <details className="memory-context-badge__details">
                    <summary
                      className="memory-context-badge__preview"
                      title={item.content}
                      aria-label={formatLabel(labels.expandItem, {
                        index: itemNumber,
                        type: typeLabel,
                      })}
                    >
                      {preview}
                    </summary>
                    <div className="memory-context-badge__full-content">
                      <span className="memory-context-badge__full-content-label">
                        {labels.fullContent}
                      </span>
                      <p>{item.content}</p>
                    </div>
                  </details>
                ) : (
                  <p
                    className="memory-context-badge__preview"
                    title={item.content}
                  >
                    {item.content || labels.emptyContent}
                  </p>
                )}
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
