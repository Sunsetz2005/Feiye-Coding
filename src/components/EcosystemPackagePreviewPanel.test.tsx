// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ECOSYSTEM_MANIFEST_MAX_BYTES,
  EcosystemPackagePreviewPanel,
  type EcosystemPackageImportPreviewV1,
  type EcosystemPackageManifestV1,
  type EcosystemPackageManifestValidationV1,
  type EcosystemPackagePreviewPanelLabels,
} from "@/components/EcosystemPackagePreviewPanel";

afterEach(cleanup);

const labels: EcosystemPackagePreviewPanelLabels = {
  regionLabel: "Ecosystem package review",
  title: "Package metadata review",
  metadataOnly: "Metadata only",
  explicitReviewBoundary: "Every package requires explicit review.",
  noExecutionDownloadInstall:
    "This preview does not execute, download, or install anything.",
  manifestJson: "Manifest JSON",
  manifestPlaceholder: "Paste one manifest or a dependency-closure array",
  inputBytes: "{bytes} / {maxBytes} bytes",
  inputTooLarge: "Input was limited to {maxBytes} bytes.",
  invalidJson: "Invalid JSON: {error}",
  invalidManifestShape: "Expected a manifest object or an array of objects.",
  packageCountOutOfRange: "Expected between 1 and {maxPackages} packages.",
  validate: "Validate",
  validating: "Validating…",
  preview: "Preview",
  previewing: "Previewing…",
  validationResults: "Validation results",
  valid: "Valid",
  invalid: "Invalid",
  canonicalManifestHash: "Canonical manifest hash",
  validationErrors: "Validation errors",
  noValidationErrors: "No validation errors",
  importPreview: "Import preview",
  previewHash: "Preview hash",
  dependencyClosure: "Dependency closure: {count} packages, dependency first",
  requiresExplicitReview: "Requires explicit review",
  reviewRequired: "Review required",
  reviewFlagMissing: "Review flag missing",
  package: "Package",
  packageReference: "Package reference",
  kind: "Kind",
  version: "Version",
  publisher: "Publisher",
  publisherVerification: "Publisher verification claim",
  source: "Source",
  sourceRevision: "Source revision",
  license: "License",
  dependencies: "Dependencies",
  dependencyItem: "{packageId} {requirement} · {kind}:{sourceId}",
  noDependencies: "No dependencies",
  permissions: "Permissions",
  noPermissions: "No permissions",
  interfaceHash: "Interface hash",
  artifactHash: "Artifact hash",
  capabilityExports: "Capability exports",
  capabilityItem: "{id} · {kind} · {version} · {hash}",
  implementationOmitted: "Implementation omitted",
  yes: "Yes",
  no: "No",
  unavailable: "Unavailable",
  hostError: "Host error: {error}",
  packageKindLabels: {
    skill: "Skill",
    plugin: "Plugin",
    mcp: "MCP",
  },
  publisherVerificationLabels: {
    verified: "Package claims verified",
    unverified: "Unverified",
  },
  sourceKindLabels: {
    built_in: "Built in",
    local: "Local",
    git: "Git",
    registry: "Registry",
    remote: "Remote",
  },
  permissionLabels: {
    filesystem_read: "Filesystem read",
    filesystem_write: "Filesystem write",
    network: "Network",
    process_spawn: "Process spawn",
    mcp_invoke: "MCP invoke",
    secrets_use: "Secrets use",
    user_interaction: "User interaction",
  },
  capabilityKindLabels: {
    skill: "Skill",
    tool: "Tool",
    resource: "Resource",
    mcp_server: "MCP server",
  },
};

function hash(character: string) {
  return character.repeat(64);
}

function manifest(
  name: string,
  overrides: Partial<EcosystemPackageManifestV1> = {},
): EcosystemPackageManifestV1 {
  return {
    schema: "sunsetz.ecosystem-package.v1",
    schemaVersion: 1,
    kind: "plugin",
    packageId: `acme/${name}`,
    displayName: `${name} package`,
    version: "1.2.3",
    publisher: {
      id: "acme",
      displayName: "Acme Publisher",
      verificationState: "verified",
      verificationAuthority: "sunsetz.trust",
      verificationEvidenceHash: hash("e"),
    },
    license: "MIT",
    source: {
      identity: { kind: "registry", sourceId: "registry.one" },
      revision: `sha256:${hash("r")}`,
    },
    interfaceHash: hash("i"),
    artifactHash: hash(name === "base" ? "a" : "b"),
    dependencies: [],
    permissions: ["mcp_invoke"],
    capabilityExports: [
      {
        capabilityId: `${name}.search`,
        kind: "tool",
        version: "1.0.0",
        interfaceHash: hash("c"),
      },
    ],
    implementationOmitted: true,
    ...overrides,
  };
}

function validValidation(
  manifestHash: string,
): EcosystemPackageManifestValidationV1 {
  return {
    version: 1,
    valid: true,
    canonicalManifestHash: manifestHash,
    errors: [],
  };
}

function previewResult(
  base: EcosystemPackageManifestV1,
  app: EcosystemPackageManifestV1,
): EcosystemPackageImportPreviewV1 {
  return {
    version: 1,
    previewHash: hash("p"),
    packages: [
      {
        packageRef: "registry:registry.one/acme/base@1.2.3",
        kind: base.kind,
        packageId: base.packageId,
        displayName: base.displayName,
        version: base.version,
        publisherId: base.publisher.id,
        publisherVerificationState: base.publisher.verificationState,
        source: base.source.identity,
        manifestHash: hash("1"),
        artifactHash: base.artifactHash,
        dependencies: [],
        permissions: base.permissions,
        capabilityExports: base.capabilityExports,
        requiresExplicitReview: true,
      },
      {
        packageRef: "registry:registry.one/acme/app@1.2.3",
        kind: app.kind,
        packageId: app.packageId,
        displayName: app.displayName,
        version: app.version,
        publisherId: app.publisher.id,
        publisherVerificationState: app.publisher.verificationState,
        source: app.source.identity,
        manifestHash: hash("2"),
        artifactHash: app.artifactHash,
        dependencies: ["registry:registry.one/acme/base@1.2.3"],
        permissions: app.permissions,
        capabilityExports: app.capabilityExports,
        requiresExplicitReview: true,
      },
    ],
  };
}

describe("EcosystemPackagePreviewPanel", () => {
  it("rejects invalid JSON locally without invoking Host callbacks", async () => {
    const user = userEvent.setup();
    const onValidate = vi.fn();
    const onPreview = vi.fn();
    render(
      <EcosystemPackagePreviewPanel
        labels={labels}
        onValidate={onValidate}
        onPreview={onPreview}
      />,
    );
    fireEvent.change(screen.getByLabelText("Manifest JSON"), {
      target: { value: "{not-json" },
    });

    await user.click(screen.getByRole("button", { name: "Validate" }));

    expect(screen.getByRole("alert").textContent).toContain("Invalid JSON");
    expect(onValidate).not.toHaveBeenCalled();
    expect(onPreview).not.toHaveBeenCalled();
  });

  it("shows validation errors and renders hostile metadata as inert text", async () => {
    const user = userEvent.setup();
    const hostile = '<img src=x onerror="window.__owned=true"><script>x</script>';
    const value = manifest("unsafe", { displayName: hostile });
    const onValidate = vi.fn().mockResolvedValue({
      version: 1,
      valid: false,
      canonicalManifestHash: null,
      errors: [hostile, "license must be one supported SPDX id"],
    });
    const onPreview = vi.fn();
    const { container } = render(
      <EcosystemPackagePreviewPanel
        labels={labels}
        initialJson={JSON.stringify(value)}
        onValidate={onValidate}
        onPreview={onPreview}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Validate" }));
    await waitFor(() => expect(onValidate).toHaveBeenCalledTimes(1));

    expect(screen.getAllByText(hostile).length).toBeGreaterThan(0);
    expect(
      screen.getByText("license must be one supported SPDX id"),
    ).toBeTruthy();
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("script")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Preview" }).hasAttribute("disabled"),
    ).toBe(true);
    expect(onPreview).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /install/i })).toBeNull();
  });

  it("builds a preview-only dependency closure with review and risk metadata", async () => {
    const user = userEvent.setup();
    const base = manifest("base");
    const app = manifest("app", {
      dependencies: [
        {
          packageId: base.packageId,
          source: base.source.identity,
          versionRequirement: "^1.0.0",
        },
      ],
      permissions: ["network", "process_spawn", "secrets_use"],
    });
    const onValidate = vi
      .fn()
      .mockResolvedValueOnce(validValidation(hash("1")))
      .mockResolvedValueOnce(validValidation(hash("2")));
    const onPreview = vi.fn().mockResolvedValue(previewResult(base, app));
    render(
      <EcosystemPackagePreviewPanel
        labels={labels}
        initialJson={JSON.stringify([base, app])}
        onValidate={onValidate}
        onPreview={onPreview}
      />,
    );

    expect(screen.getByText("Metadata only")).toBeTruthy();
    expect(
      screen.getByText(
        "This preview does not execute, download, or install anything.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: /install/i })).toBeNull();

    await user.click(screen.getByRole("button", { name: "Validate" }));
    await waitFor(() => expect(onValidate).toHaveBeenCalledTimes(2));
    expect(screen.getAllByText("Acme Publisher · acme").length).toBe(2);
    expect(screen.getAllByText("MIT").length).toBe(2);
    expect(screen.getAllByText(app.artifactHash).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Preview" }));
    await waitFor(() => expect(onPreview).toHaveBeenCalledTimes(1));
    expect(onPreview).toHaveBeenCalledWith({
      version: 1,
      candidates: [
        { manifest: base, expectedManifestHash: hash("1") },
        { manifest: app, expectedManifestHash: hash("2") },
      ],
    });

    const result = screen.getByRole("region", { name: "Import preview" });
    expect(
      within(result).getByText(
        "Dependency closure: 2 packages, dependency first",
      ),
    ).toBeTruthy();
    expect(within(result).getAllByText("Review required").length).toBe(2);
    expect(within(result).getByText("Network")).toBeTruthy();
    expect(within(result).getByText("Process spawn")).toBeTruthy();
    expect(within(result).getByText("Secrets use")).toBeTruthy();
    expect(
      within(result).getByText("registry:registry.one/acme/base@1.2.3"),
    ).toBeTruthy();
    expect(within(result).getAllByText(app.artifactHash).length).toBeGreaterThan(
      0,
    );
    expect(result.textContent?.match(/License: MIT/g)?.length).toBe(2);
    expect(screen.queryByRole("button", { name: /install/i })).toBeNull();
  });

  it("honors disabled state for input and both review steps", () => {
    render(
      <EcosystemPackagePreviewPanel
        labels={labels}
        initialJson={JSON.stringify(manifest("disabled"))}
        disabled
        onValidate={vi.fn()}
        onPreview={vi.fn()}
      />,
    );

    expect(
      screen.getByLabelText("Manifest JSON").hasAttribute("disabled"),
    ).toBe(true);
    expect(
      screen.getByRole("button", { name: "Validate" }).hasAttribute("disabled"),
    ).toBe(true);
    expect(
      screen.getByRole("button", { name: "Preview" }).hasAttribute("disabled"),
    ).toBe(true);
  });

  it("locks the form while validation is busy", async () => {
    const user = userEvent.setup();
    let resolveValidation: (
      value: EcosystemPackageManifestValidationV1,
    ) => void = () => {};
    const onValidate = vi.fn(
      () =>
        new Promise<EcosystemPackageManifestValidationV1>((resolve) => {
          resolveValidation = resolve;
        }),
    );
    render(
      <EcosystemPackagePreviewPanel
        labels={labels}
        initialJson={JSON.stringify(manifest("busy"))}
        onValidate={onValidate}
        onPreview={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(screen.getByRole("button", { name: "Validating…" })).toBeTruthy();
    expect(
      screen.getByLabelText("Manifest JSON").hasAttribute("disabled"),
    ).toBe(true);
    expect(
      screen.getByRole("button", { name: "Preview" }).hasAttribute("disabled"),
    ).toBe(true);

    await act(async () => {
      resolveValidation(validValidation(hash("v")));
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Validate" })).toBeTruthy(),
    );
  });

  it("bounds UTF-8 input to the same 256 KiB Host limit", () => {
    render(
      <EcosystemPackagePreviewPanel
        labels={labels}
        onValidate={vi.fn()}
        onPreview={vi.fn()}
      />,
    );
    const input = screen.getByLabelText("Manifest JSON") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "😀".repeat(70_000) } });

    expect(new TextEncoder().encode(input.value).length).toBeLessThanOrEqual(
      ECOSYSTEM_MANIFEST_MAX_BYTES,
    );
    expect(screen.getByRole("alert").textContent).toContain(
      String(ECOSYSTEM_MANIFEST_MAX_BYTES),
    );
  });
});
