import { t, type Locale, type MessageKey } from "../i18n";

export interface AppCommandError {
  code: string;
  message: string;
  retryable: boolean;
  fieldErrors?: Record<string, string>;
  diagnostic?: string;
}

type ErrorLike = {
  code?: unknown;
  message?: unknown;
  detail?: unknown;
  retryable?: unknown;
  fieldErrors?: unknown;
};

const STATUS_CODE_RE = /\b(400|401|403|404|408|409|413|422|429|5\d\d)\b/;

function asRecord(value: unknown): ErrorLike | null {
  return value != null && typeof value === "object"
    ? (value as ErrorLike)
    : null;
}

function parseJsonObject(value: string): ErrorLike | null {
  const trimmed = value.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return null;
  try {
    return asRecord(JSON.parse(trimmed));
  } catch {
    return null;
  }
}

function diagnosticText(error: unknown): string {
  if (error instanceof Error) return error.stack || error.message;
  if (typeof error === "string") return error;
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function textFrom(value: unknown): string {
  if (typeof value === "string") return value.trim();
  if (value == null) return "";
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function messageFor(
  locale: Locale,
  key: Extract<
    MessageKey,
    | "error.command.invalid"
    | "error.command.payloadTooLarge"
    | "error.command.permission"
    | "error.command.failed"
  >,
): string {
  return t(locale, key);
}

/**
 * Convert Tauri, HTTP and legacy string failures into one UI-safe shape.
 * Raw provider payloads remain available as diagnostic detail only.
 */
export function normalizeAppCommandError(
  error: unknown,
  locale: Locale = "zh",
): AppCommandError {
  const diagnostic = diagnosticText(error);
  const direct = asRecord(error);
  const rawMessage =
    textFrom(direct?.message) ||
    (error instanceof Error ? error.message : textFrom(error));
  const parsed = parseJsonObject(rawMessage);
  const source = parsed ?? direct;
  const detail = textFrom(source?.detail);
  const combined = `${textFrom(source?.code)} ${rawMessage} ${detail}`.trim();
  const lower = combined.toLowerCase();
  const status = combined.match(STATUS_CODE_RE)?.[1] ?? null;

  let code = textFrom(source?.code).toUpperCase();
  let messageKey:
    | "error.command.invalid"
    | "error.command.payloadTooLarge"
    | "error.command.permission"
    | "error.command.failed" = "error.command.failed";
  let retryable = source?.retryable === true;

  if (
    status === "413" ||
    /payload too large|request entity too large|too large|超出.*大小/.test(lower)
  ) {
    code = code || "PAYLOAD_TOO_LARGE";
    messageKey = "error.command.payloadTooLarge";
    retryable = false;
  } else if (
    status === "400" ||
    status === "404" ||
    status === "409" ||
    status === "422" ||
    /bad request|invalid request|unprocessable|provider.*config|参数.*无效/.test(
      lower,
    )
  ) {
    code = code || "REQUEST_INVALID";
    messageKey = "error.command.invalid";
    retryable = false;
  } else if (
    status === "401" ||
    status === "403" ||
    /unauthori[sz]ed|forbidden|permission denied|权限|未授权/.test(lower)
  ) {
    code = code || "PERMISSION_DENIED";
    messageKey = "error.command.permission";
    retryable = false;
  } else if (
    status === "408" ||
    status === "429" ||
    status?.startsWith("5") ||
    /timeout|timed out|temporar|network|connection reset|rate.?limit/.test(lower)
  ) {
    code = code || "REQUEST_FAILED";
    retryable = true;
  } else {
    code = code || "COMMAND_FAILED";
  }

  const fieldErrors =
    source?.fieldErrors &&
    typeof source.fieldErrors === "object" &&
    !Array.isArray(source.fieldErrors)
      ? Object.fromEntries(
          Object.entries(source.fieldErrors as Record<string, unknown>).map(
            ([key, value]) => [key, textFrom(value)],
          ),
        )
      : undefined;

  return {
    code,
    message: messageFor(locale, messageKey),
    retryable,
    ...(fieldErrors && Object.keys(fieldErrors).length > 0
      ? { fieldErrors }
      : {}),
    ...(diagnostic ? { diagnostic } : {}),
  };
}
