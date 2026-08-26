import { useMemo } from "react";
import { IconChevronRight, IconPlan } from "@/components/icons";
import { MarkdownBody } from "@/components/MarkdownBody";
import {
  planArtifactProductStatus,
  type PlanArtifactStatusV1,
} from "@/lib/planArtifacts";
import { buildPlanArtifactPreview } from "./planArtifact";
import "./workbench-content.css";

export interface PlanArtifactCardModel {
  visible?: boolean;
  title: string;
  body: string;
  entries?: unknown[];
  waiting?: boolean;
  artifactStatus?: PlanArtifactStatusV1 | null;
}

export interface PlanArtifactCardLabels {
  plan: string;
  empty: string;
  open: string;
  approved: string;
  executing: string;
  done: string;
}

export interface PlanArtifactCardProps {
  artifact: PlanArtifactCardModel;
  labels: PlanArtifactCardLabels;
  onOpen?: () => void;
}

export function PlanArtifactCard({
  artifact,
  labels,
  onOpen,
}: PlanArtifactCardProps) {
  const preview = useMemo(
    () => buildPlanArtifactPreview(artifact.body, artifact.entries),
    [artifact.body, artifact.entries],
  );

  if (artifact.visible === false) return null;

  const product = planArtifactProductStatus(artifact.artifactStatus);
  const status =
    product === "approved"
      ? labels.approved
      : product === "executing"
        ? labels.executing
        : product === "done"
          ? labels.done
          : labels.plan;
  return (
    <article
      className="plan-artifact-card"
      aria-label={artifact.title || status}
      data-testid="plan-artifact-card"
    >
      <header className="plan-artifact-card__header">
        <span className="plan-artifact-card__icon" aria-hidden>
          <IconPlan size={15} />
        </span>
        <div className="plan-artifact-card__heading">
          <h3 className="plan-artifact-card__title">
            {artifact.title || status}
          </h3>
          <span className="plan-artifact-card__status">{status}</span>
        </div>
        {onOpen ? (
          <button
            type="button"
            className="plan-artifact-card__open"
            onClick={onOpen}
          >
            <span>{labels.open}</span>
            <IconChevronRight size={15} />
          </button>
        ) : null}
      </header>

      <div
        className={
          "plan-artifact-card__preview" +
          (preview.truncated ? " is-truncated" : "")
        }
      >
        {preview.preview ? (
          <MarkdownBody>{preview.preview}</MarkdownBody>
        ) : (
          <p className="plan-artifact-card__empty">{labels.empty}</p>
        )}
      </div>
    </article>
  );
}
