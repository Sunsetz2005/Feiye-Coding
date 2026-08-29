/**
 * Split streaming markdown so completed blocks stay frozen.
 * Only the live tail is re-parsed / drip-revealed on each token.
 */

export type StreamingMarkdownParts = {
  frozen: string;
  tail: string;
};

const FENCE = "```";

function lineStartsFence(src: string, index: number): boolean {
  if (!src.startsWith(FENCE, index)) return false;
  return index === 0 || src[index - 1] === "\n";
}

/**
 * Freeze through the last paragraph break (`\\n\\n`) or closed fence that is
 * not inside an open fence. Incomplete fences stay in `tail`.
 */
export function splitStreamingMarkdown(src: string): StreamingMarkdownParts {
  if (!src) return { frozen: "", tail: "" };

  let inFence = false;
  let lastFreezeAt = 0;
  let i = 0;

  while (i < src.length) {
    if (lineStartsFence(src, i)) {
      const lineEnd = src.indexOf("\n", i);
      const next = lineEnd === -1 ? src.length : lineEnd + 1;
      inFence = !inFence;
      i = next;
      if (!inFence) lastFreezeAt = i;
      continue;
    }
    if (!inFence && src.startsWith("\n\n", i)) {
      lastFreezeAt = i + 2;
      i += 2;
      continue;
    }
    i += 1;
  }

  if (lastFreezeAt <= 0 || lastFreezeAt >= src.length) {
    return { frozen: "", tail: src };
  }
  return {
    frozen: src.slice(0, lastFreezeAt),
    tail: src.slice(lastFreezeAt),
  };
}
