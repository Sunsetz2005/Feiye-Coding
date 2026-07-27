/** Private browser-side constants for the upstream runtime compatibility boundary. */
export const LEGACY_THEME_STORAGE_KEY = "grok-app.theme";
export const RUNTIME_USAGE_URL = "https://grok.com/?_s=usage";
export const RUNTIME_SUBSCRIBE_URL =
  "https://grok.com/supergrok?referrer=grok-build";

export function upstreamSubscriptionKind(
  raw: string,
): "heavy" | "sunsetz-pro" | null {
  const compact = raw.trim().toLowerCase().replace(/[\s_-]+/g, "");
  if (!compact) return null;
  if (compact.includes("heavy") || compact === "supergrokpro") return "heavy";
  if (
    compact.includes("supergrok") ||
    compact.includes("grokpro") ||
    compact.includes("xpremium") ||
    compact.includes("premiumplus")
  ) {
    return "sunsetz-pro";
  }
  return null;
}
