import { useState, type ChangeEvent } from "react";
import type {
  EcosystemCapabilityKindV1,
  EcosystemPackageImportPreviewItemV1,
  EcosystemPackageImportPreviewRequestV1,
  EcosystemPackageImportPreviewV1,
  EcosystemPackageKindV1,
  EcosystemPackageManifestValidationV1,
  EcosystemPackageManifestV1,
  EcosystemPackagePermissionV1,
  EcosystemPublisherVerificationStateV1,
  EcosystemSourceKindV1,
} from "@/lib/api";

export type {
  EcosystemPackageImportPreviewRequestV1,
  EcosystemPackageImportPreviewV1,
  EcosystemPackageManifestValidationV1,
  EcosystemPackageManifestV1,
} from "@/lib/api";

export const ECOSYSTEM_MANIFEST_MAX_BYTES = 256 * 1024;
const ECOSYSTEM_PREVIEW_MAX_PACKAGES = 128;

export type EcosystemPackagePreviewPanelLabels = {
  regionLabel: string;
  title: string;
  metadataOnly: string;
  explicitReviewBoundary: string;
  noExecutionDownloadInstall: string;
  manifestJson: string;
  manifestPlaceholder: string;
  inputBytes: string;
  inputTooLarge: string;
  invalidJson: string;
  invalidManifestShape: string;
  packageCountOutOfRange: string;
  validate: string;
  validating: string;
  preview: string;
  previewing: string;
  validationResults: string;
  valid: string;
  invalid: string;
  canonicalManifestHash: string;
  validationErrors: string;
  noValidationErrors: string;
  importPreview: string;
  previewHash: string;
  dependencyClosure: string;
  requiresExplicitReview: string;
  reviewRequired: string;
  reviewFlagMissing: string;
  package: string;
  packageReference: string;
  kind: string;
  version: string;
  publisher: string;
  publisherVerification: string;
  source: string;
  sourceRevision: string;
  license: string;
  dependencies: string;
  dependencyItem: string;
  noDependencies: string;
  permissions: string;
  noPermissions: string;
  interfaceHash: string;
  artifactHash: string;
  capabilityExports: string;
  capabilityItem: string;
  implementationOmitted: string;
  yes: string;
  no: string;
  unavailable: string;
  hostError: string;
  packageKindLabels: Record<EcosystemPackageKindV1, string>;
  publisherVerificationLabels: Record<
    EcosystemPublisherVerificationStateV1,
    string
  >;
  sourceKindLabels: Record<EcosystemSourceKindV1, string>;
  permissionLabels: Record<EcosystemPackagePermissionV1, string>;
  capabilityKindLabels: Record<EcosystemCapabilityKindV1, string>;
};

export type EcosystemPackagePreviewPanelProps = {
  labels: EcosystemPackagePreviewPanelLabels;
  initialJson?: string;
  disabled?: boolean;
  onValidate: (
    manifest: EcosystemPackageManifestV1,
  ) =>
    | EcosystemPackageManifestValidationV1
    | Promise<EcosystemPackageManifestValidationV1>;
  onPreview: (
    request: EcosystemPackageImportPreviewRequestV1,
  ) =>
    | EcosystemPackageImportPreviewV1
    | Promise<EcosystemPackageImportPreviewV1>;
};

type ValidatedManifest = {
  manifest: EcosystemPackageManifestV1;
  validation: EcosystemPackageManifestValidationV1;
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

function utf8Length(value: string) {
  return new TextEncoder().encode(value).length;
}

function truncateUtf8(value: string, maxBytes: number) {
  let used = 0;
  let bounded = "";
  const encoder = new TextEncoder();
  for (const character of value) {
    const bytes = encoder.encode(character).length;
    if (used + bytes > maxBytes) break;
    bounded += character;
    used += bytes;
  }
  return bounded;
}

function isJsonObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function manifestIdentity(manifest: EcosystemPackageManifestV1) {
  return [
    manifest.packageId,
    manifest.version,
    manifest.source?.identity?.kind,
    manifest.source?.identity?.sourceId,
  ].join("\u0000");
}

function previewIdentity(item: EcosystemPackageImportPreviewItemV1) {
  return [item.packageId, item.version, item.source.kind, item.source.sourceId].join(
    "\u0000",
  );
}

function safeText(value: unknown, fallback: string) {
  return typeof value === "string" && value.length > 0 ? value : fallback;
}

function MetadataList({
  label,
  items,
  empty,
}: {
  label: string;
  items: string[];
  empty: string;
}) {
  return (
    <div className="ecosystem-package-preview__metadata-list">
      <dt>{label}</dt>
      <dd>
        {items.length > 0 ? (
          <ul>
            {items.map((item, index) => (
              <li key={`${index}:${item}`}>{item}</li>
            ))}
          </ul>
        ) : (
          empty
        )}
      </dd>
    </div>
  );
}

function ManifestMetadata({
  manifest,
  labels,
}: {
  manifest: EcosystemPackageManifestV1;
  labels: EcosystemPackagePreviewPanelLabels;
}) {
  const dependencies = Array.isArray(manifest.dependencies)
    ? manifest.dependencies.map((dependency) =>
        formatLabel(labels.dependencyItem, {
          packageId: safeText(dependency?.packageId, labels.unavailable),
          requirement: safeText(
            dependency?.versionRequirement,
            labels.unavailable,
          ),
          kind:
            labels.sourceKindLabels[dependency?.source?.kind] ??
            labels.unavailable,
          sourceId: safeText(dependency?.source?.sourceId, labels.unavailable),
        }),
      )
    : [];
  const permissions = Array.isArray(manifest.permissions)
    ? manifest.permissions.map(
        (permission) => labels.permissionLabels[permission] ?? permission,
      )
    : [];
  const capabilities = Array.isArray(manifest.capabilityExports)
    ? manifest.capabilityExports.map((capability) =>
        formatLabel(labels.capabilityItem, {
          id: safeText(capability?.capabilityId, labels.unavailable),
          kind:
            labels.capabilityKindLabels[capability?.kind] ?? labels.unavailable,
          version: safeText(capability?.version, labels.unavailable),
          hash: safeText(capability?.interfaceHash, labels.unavailable),
        }),
      )
    : [];

  return (
    <dl className="ecosystem-package-preview__metadata">
      <div>
        <dt>{labels.publisher}</dt>
        <dd>
          {safeText(manifest.publisher?.displayName, labels.unavailable)} ·{" "}
          {safeText(manifest.publisher?.id, labels.unavailable)}
        </dd>
      </div>
      <div>
        <dt>{labels.publisherVerification}</dt>
        <dd>
          {labels.publisherVerificationLabels[
            manifest.publisher?.verificationState
          ] ?? labels.unavailable}
        </dd>
      </div>
      <div>
        <dt>{labels.source}</dt>
        <dd>
          {labels.sourceKindLabels[manifest.source?.identity?.kind] ??
            labels.unavailable}
          :{safeText(manifest.source?.identity?.sourceId, labels.unavailable)}
        </dd>
      </div>
      <div>
        <dt>{labels.sourceRevision}</dt>
        <dd>{safeText(manifest.source?.revision, labels.unavailable)}</dd>
      </div>
      <div>
        <dt>{labels.license}</dt>
        <dd>{safeText(manifest.license, labels.unavailable)}</dd>
      </div>
      <MetadataList
        label={labels.dependencies}
        items={dependencies}
        empty={labels.noDependencies}
      />
      <MetadataList
        label={labels.permissions}
        items={permissions}
        empty={labels.noPermissions}
      />
      <div>
        <dt>{labels.interfaceHash}</dt>
        <dd>
          <code>{safeText(manifest.interfaceHash, labels.unavailable)}</code>
        </dd>
      </div>
      <div>
        <dt>{labels.artifactHash}</dt>
        <dd>
          <code>{safeText(manifest.artifactHash, labels.unavailable)}</code>
        </dd>
      </div>
      <MetadataList
        label={labels.capabilityExports}
        items={capabilities}
        empty={labels.unavailable}
      />
      <div>
        <dt>{labels.implementationOmitted}</dt>
        <dd>{manifest.implementationOmitted ? labels.yes : labels.no}</dd>
      </div>
    </dl>
  );
}

export function EcosystemPackagePreviewPanel({
  labels,
  initialJson = "",
  disabled = false,
  onValidate,
  onPreview,
}: EcosystemPackagePreviewPanelProps) {
  const [manifestJson, setManifestJson] = useState(() =>
    truncateUtf8(initialJson, ECOSYSTEM_MANIFEST_MAX_BYTES),
  );
  const [inputError, setInputError] = useState<string | null>(() =>
    utf8Length(initialJson) > ECOSYSTEM_MANIFEST_MAX_BYTES
      ? formatLabel(labels.inputTooLarge, {
          maxBytes: ECOSYSTEM_MANIFEST_MAX_BYTES,
        })
      : null,
  );
  const [parseError, setParseError] = useState<string | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [validated, setValidated] = useState<ValidatedManifest[] | null>(null);
  const [preview, setPreview] =
    useState<EcosystemPackageImportPreviewV1 | null>(null);
  const [busy, setBusy] = useState<"validate" | "preview" | null>(null);

  const resetDerivedState = () => {
    setParseError(null);
    setOperationError(null);
    setValidated(null);
    setPreview(null);
  };

  const changeManifestJson = (event: ChangeEvent<HTMLTextAreaElement>) => {
    const incoming = event.target.value;
    if (utf8Length(incoming) > ECOSYSTEM_MANIFEST_MAX_BYTES) {
      setManifestJson(truncateUtf8(incoming, ECOSYSTEM_MANIFEST_MAX_BYTES));
      setInputError(
        formatLabel(labels.inputTooLarge, {
          maxBytes: ECOSYSTEM_MANIFEST_MAX_BYTES,
        }),
      );
    } else {
      setManifestJson(incoming);
      setInputError(null);
    }
    resetDerivedState();
  };

  const parseManifests = () => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(manifestJson);
    } catch (reason) {
      setParseError(
        formatLabel(labels.invalidJson, {
          error: reason instanceof Error ? reason.message : String(reason),
        }),
      );
      return null;
    }
    const values = Array.isArray(parsed) ? parsed : [parsed];
    if (
      values.length === 0 ||
      values.length > ECOSYSTEM_PREVIEW_MAX_PACKAGES
    ) {
      setParseError(
        formatLabel(labels.packageCountOutOfRange, {
          maxPackages: ECOSYSTEM_PREVIEW_MAX_PACKAGES,
        }),
      );
      return null;
    }
    if (!values.every(isJsonObject)) {
      setParseError(labels.invalidManifestShape);
      return null;
    }
    setParseError(null);
    return values as unknown as EcosystemPackageManifestV1[];
  };

  const validate = async () => {
    if (disabled || busy || !manifestJson.trim()) return;
    resetDerivedState();
    const manifests = parseManifests();
    if (!manifests) return;
    setBusy("validate");
    try {
      const rows: ValidatedManifest[] = [];
      for (const manifest of manifests) {
        rows.push({ manifest, validation: await onValidate(manifest) });
      }
      setValidated(rows);
    } catch (reason) {
      setOperationError(
        formatLabel(labels.hostError, {
          error: reason instanceof Error ? reason.message : String(reason),
        }),
      );
    } finally {
      setBusy(null);
    }
  };

  const canPreview =
    validated !== null &&
    validated.length > 0 &&
    validated.every(
      ({ validation }) =>
        validation.valid && Boolean(validation.canonicalManifestHash),
    );

  const buildPreview = async () => {
    if (disabled || busy || !canPreview || !validated) return;
    setBusy("preview");
    setOperationError(null);
    setPreview(null);
    const request: EcosystemPackageImportPreviewRequestV1 = {
      version: 1,
      candidates: validated.map(({ manifest, validation }) => ({
        manifest,
        expectedManifestHash: validation.canonicalManifestHash as string,
      })),
    };
    try {
      setPreview(await onPreview(request));
    } catch (reason) {
      setOperationError(
        formatLabel(labels.hostError, {
          error: reason instanceof Error ? reason.message : String(reason),
        }),
      );
    } finally {
      setBusy(null);
    }
  };

  const manifestsByIdentity = new Map(
    (validated ?? []).map(({ manifest }) => [manifestIdentity(manifest), manifest]),
  );

  return (
    <section
      className="settings-card ext-card ecosystem-package-preview"
      aria-label={labels.regionLabel}
      data-testid="ecosystem-package-preview-panel"
    >
      <header className="ecosystem-package-preview__header">
        <h2>{labels.title}</h2>
        <p>
          <strong>{labels.metadataOnly}</strong>
        </p>
        <p>{labels.explicitReviewBoundary}</p>
        <p>{labels.noExecutionDownloadInstall}</p>
      </header>

      <div className="ecosystem-package-preview__input">
        <label htmlFor="ecosystem-package-manifest-json">
          {labels.manifestJson}
        </label>
        <textarea
          id="ecosystem-package-manifest-json"
          value={manifestJson}
          disabled={disabled || busy !== null}
          maxLength={ECOSYSTEM_MANIFEST_MAX_BYTES}
          rows={12}
          spellCheck={false}
          placeholder={labels.manifestPlaceholder}
          onChange={changeManifestJson}
        />
        <div className="ecosystem-package-preview__byte-count">
          {formatLabel(labels.inputBytes, {
            bytes: utf8Length(manifestJson),
            maxBytes: ECOSYSTEM_MANIFEST_MAX_BYTES,
          })}
        </div>
        {inputError ? <div role="alert">{inputError}</div> : null}
        {parseError ? <div role="alert">{parseError}</div> : null}
        {operationError ? <div role="alert">{operationError}</div> : null}
        <div className="ecosystem-package-preview__actions">
          <button
            type="button"
            className="btn btn--secondary btn--sm"
            disabled={
              disabled || busy !== null || manifestJson.trim().length === 0
            }
            onClick={() => void validate()}
          >
            {busy === "validate" ? labels.validating : labels.validate}
          </button>
          <button
            type="button"
            className="btn btn--primary btn--sm"
            disabled={disabled || busy !== null || !canPreview}
            onClick={() => void buildPreview()}
          >
            {busy === "preview" ? labels.previewing : labels.preview}
          </button>
        </div>
      </div>

      {validated ? (
        <section
          className="ecosystem-package-preview__validation"
          aria-label={labels.validationResults}
        >
          <h2>{labels.validationResults}</h2>
          {validated.map(({ manifest, validation }, index) => (
            <article
              className="ecosystem-package-preview__package"
              key={`${index}:${manifestIdentity(manifest)}`}
            >
              <h3>
                {safeText(manifest.displayName, labels.unavailable)} ·{" "}
                {validation.valid ? labels.valid : labels.invalid}
              </h3>
              <div>
                {labels.package}: {safeText(manifest.packageId, labels.unavailable)}
              </div>
              <div>
                {labels.kind}:{" "}
                {labels.packageKindLabels[manifest.kind] ?? labels.unavailable}
              </div>
              <div>
                {labels.version}: {safeText(manifest.version, labels.unavailable)}
              </div>
              <div>
                {labels.canonicalManifestHash}:{" "}
                <code>
                  {safeText(
                    validation.canonicalManifestHash,
                    labels.unavailable,
                  )}
                </code>
              </div>
              <ManifestMetadata manifest={manifest} labels={labels} />
              <div className="ecosystem-package-preview__validation-errors">
                <strong>{labels.validationErrors}</strong>
                {validation.errors.length > 0 ? (
                  <ul>
                    {validation.errors.map((error, errorIndex) => (
                      <li key={`${errorIndex}:${error}`}>{error}</li>
                    ))}
                  </ul>
                ) : (
                  <p>{labels.noValidationErrors}</p>
                )}
              </div>
            </article>
          ))}
        </section>
      ) : null}

      {preview ? (
        <section
          className="ecosystem-package-preview__result"
          aria-label={labels.importPreview}
        >
          <h2>{labels.importPreview}</h2>
          <div>
            {labels.previewHash}: <code>{preview.previewHash}</code>
          </div>
          <div>
            {formatLabel(labels.dependencyClosure, {
              count: preview.packages.length,
            })}
          </div>
          <ol>
            {preview.packages.map((item, index) => {
              const manifest = manifestsByIdentity.get(previewIdentity(item));
              const permissions = item.permissions.map(
                (permission) => labels.permissionLabels[permission] ?? permission,
              );
              const capabilities = item.capabilityExports.map((capability) =>
                formatLabel(labels.capabilityItem, {
                  id: capability.capabilityId,
                  kind:
                    labels.capabilityKindLabels[capability.kind] ??
                    capability.kind,
                  version: capability.version,
                  hash: capability.interfaceHash,
                }),
              );
              return (
                <li key={`${index}:${item.packageRef}`}>
                  <article className="ecosystem-package-preview__package">
                    <h3>{item.displayName}</h3>
                    <div>
                      {labels.packageReference}: {item.packageRef}
                    </div>
                    <div>
                      {labels.kind}: {labels.packageKindLabels[item.kind]}
                    </div>
                    <div>
                      {labels.publisher}: {item.publisherId} ·{" "}
                      {
                        labels.publisherVerificationLabels[
                          item.publisherVerificationState
                        ]
                      }
                    </div>
                    <div>
                      {labels.source}: {labels.sourceKindLabels[item.source.kind]}:
                      {item.source.sourceId}
                    </div>
                    <div>
                      {labels.sourceRevision}:{" "}
                      {safeText(manifest?.source?.revision, labels.unavailable)}
                    </div>
                    <div>
                      {labels.license}: {safeText(manifest?.license, labels.unavailable)}
                    </div>
                    <div>
                      {labels.requiresExplicitReview}:{" "}
                      <strong>
                        {item.requiresExplicitReview
                          ? labels.reviewRequired
                          : labels.reviewFlagMissing}
                      </strong>
                    </div>
                    <MetadataList
                      label={labels.dependencies}
                      items={item.dependencies}
                      empty={labels.noDependencies}
                    />
                    <MetadataList
                      label={labels.permissions}
                      items={permissions}
                      empty={labels.noPermissions}
                    />
                    <div>
                      {labels.canonicalManifestHash}: <code>{item.manifestHash}</code>
                    </div>
                    <div>
                      {labels.artifactHash}: <code>{item.artifactHash}</code>
                    </div>
                    <MetadataList
                      label={labels.capabilityExports}
                      items={capabilities}
                      empty={labels.unavailable}
                    />
                  </article>
                </li>
              );
            })}
          </ol>
        </section>
      ) : null}
    </section>
  );
}
