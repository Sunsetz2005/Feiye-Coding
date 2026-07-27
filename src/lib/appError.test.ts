import { describe, expect, it } from "vitest";
import { normalizeAppCommandError } from "./appError";

describe("normalizeAppCommandError", () => {
  it("does not expose a JSON Bad Request body as the primary message", () => {
    const error = normalizeAppCommandError('{"detail":"Bad Request"}', "zh");
    expect(error.code).toBe("REQUEST_INVALID");
    expect(error.retryable).toBe(false);
    expect(error.message).not.toContain("Bad Request");
    expect(error.message).not.toContain("{");
    expect(error.diagnostic).toContain("Bad Request");
  });

  it("classifies payload and permission failures", () => {
    expect(normalizeAppCommandError("HTTP 413", "en").code).toBe(
      "PAYLOAD_TOO_LARGE",
    );
    expect(normalizeAppCommandError("HTTP 403 forbidden", "en").code).toBe(
      "PERMISSION_DENIED",
    );
  });

  it("keeps structured field errors and retryability", () => {
    const error = normalizeAppCommandError(
      {
        code: "VALIDATION",
        message: "invalid request",
        retryable: false,
        fieldErrors: { model: "required" },
      },
      "en",
    );
    expect(error.code).toBe("VALIDATION");
    expect(error.fieldErrors).toEqual({ model: "required" });
    expect(error.retryable).toBe(false);
  });

  it("marks transient failures as retryable", () => {
    expect(normalizeAppCommandError("HTTP 503 upstream unavailable", "en"))
      .toMatchObject({ retryable: true });
  });
});
