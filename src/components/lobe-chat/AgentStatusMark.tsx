/**
 * Compact live-status pill for the streaming tail.
 *
 * Geometry is MIT thinking-orbs (Jakub Antalik). Ink is remapped onto
 * Sunsetz tokens: near dots follow the MetalForge share-link paper colors,
 * depth mixes in `--accent`. No Metal/Skia shaders.
 */

import { useEffect, useRef } from "react";
import type { OrbState } from "thinking-orbs";
import { MODE_FRAMES, resolvePreset } from "thinking-orbs/engine";
import type { OrbFrame } from "thinking-orbs/engine";

const ORB_SIZE = 20;
const DPR_CAP = 2;
const NEAR_DARK: RGB = [244, 241, 234]; // #F4F1EA
const NEAR_LIGHT: RGB = [37, 36, 42]; // #25242A
const ACCENT_FALLBACK: RGB = [242, 120, 92]; // --accent dark

type RGB = [number, number, number];

function mix(a: number, b: number, t: number): number {
  return Math.round(a + (b - a) * t);
}

function parseCssColor(input: string): RGB | null {
  const value = input.trim();
  if (!value) return null;
  if (value.startsWith("#")) {
    const hex = value.slice(1);
    if (hex.length === 3) {
      return [
        parseInt(hex[0]! + hex[0]!, 16),
        parseInt(hex[1]! + hex[1]!, 16),
        parseInt(hex[2]! + hex[2]!, 16),
      ];
    }
    if (hex.length >= 6) {
      return [
        parseInt(hex.slice(0, 2), 16),
        parseInt(hex.slice(2, 4), 16),
        parseInt(hex.slice(4, 6), 16),
      ];
    }
    return null;
  }
  const rgb = value.match(
    /rgba?\(\s*([\d.]+)\s*[, ]\s*([\d.]+)\s*[, ]\s*([\d.]+)/,
  );
  if (!rgb) return null;
  return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3])];
}

function isDarkTheme(): boolean {
  if (typeof document === "undefined") return true;
  return document.documentElement.getAttribute("data-theme") !== "light";
}

function prefersReducedMotion(): boolean {
  return (
    typeof matchMedia !== "undefined" &&
    matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

function paintTinted(
  ctx: CanvasRenderingContext2D,
  frame: OrbFrame,
  dark: boolean,
  accent: RGB,
) {
  const near = dark ? NEAR_DARK : NEAR_LIGHT;
  const ink = (white: number, alpha: number | undefined) => {
    const t = Math.min(1, Math.max(0, dark ? 1 - white : white));
    const r = mix(accent[0], near[0], t);
    const g = mix(accent[1], near[1], t);
    const b = mix(accent[2], near[2], t);
    return `rgba(${r},${g},${b},${alpha ?? 1})`;
  };

  for (const line of frame.lines) {
    ctx.strokeStyle = ink(line.white, line.a);
    ctx.lineWidth = line.w;
    ctx.beginPath();
    ctx.moveTo(line.x1, line.y1);
    ctx.lineTo(line.x2, line.y2);
    ctx.stroke();
  }
  for (const dot of frame.dots) {
    ctx.fillStyle = ink(dot.white, dot.a);
    ctx.beginPath();
    ctx.arc(dot.x, dot.y, dot.r, 0, Math.PI * 2);
    ctx.fill();
  }
}

export function AgentStatusMark({
  state,
  label,
}: {
  state: OrbState;
  label: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = Math.min(
      DPR_CAP,
      typeof devicePixelRatio === "number" ? devicePixelRatio : 1,
    );
    canvas.width = Math.round(ORB_SIZE * dpr);
    canvas.height = Math.round(ORB_SIZE * dpr);

    const { mode, speed, opts } = resolvePreset(state, ORB_SIZE);
    const frameAt = MODE_FRAMES[mode];
    const dark = isDarkTheme();
    const accent =
      parseCssColor(
        getComputedStyle(canvas).getPropertyValue("--accent"),
      ) ?? ACCENT_FALLBACK;

    const draw = (elapsed: number) => {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, ORB_SIZE, ORB_SIZE);
      paintTinted(ctx, frameAt(ORB_SIZE, elapsed, opts), dark, accent);
    };

    if (prefersReducedMotion()) {
      draw(0.6);
      return;
    }

    let raf = 0;
    let running = false;
    const tick = () => {
      draw((performance.now() / 1000) * speed);
      if (running) raf = requestAnimationFrame(tick);
    };
    const start = () => {
      if (running) return;
      running = true;
      raf = requestAnimationFrame(tick);
    };
    const stop = () => {
      running = false;
      cancelAnimationFrame(raf);
    };

    draw((performance.now() / 1000) * speed);
    let visible = true;
    const io =
      typeof IntersectionObserver !== "undefined"
        ? new IntersectionObserver(([entry]) => {
            visible = Boolean(entry?.isIntersecting);
            if (visible && document.visibilityState !== "hidden") start();
            else stop();
          })
        : null;
    io?.observe(canvas);
    const onVisibility = () => {
      if (document.visibilityState === "hidden") stop();
      else if (visible) start();
    };
    document.addEventListener("visibilitychange", onVisibility);
    if (!io) start();

    return () => {
      stop();
      io?.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [state]);

  return (
    <div
      className="lobe-chat-status-pill"
      role="status"
      aria-live="polite"
      data-testid="agent-status-pill"
      data-orb-state={state}
    >
      <canvas
        ref={canvasRef}
        className="lobe-chat-status-pill__orb"
        width={ORB_SIZE}
        height={ORB_SIZE}
        aria-hidden
      />
      <span className="lobe-chat-status-pill__label">{label}</span>
    </div>
  );
}
