// Small building blocks shared by the screens.

import { useEffect, useMemo, useRef } from "react";
import type { ReactNode } from "react";

import { detectPlatform, orderButtons } from "../lib/platform";
import type { Platform } from "../lib/platform";

export type Tone = "ok" | "warn" | "bad" | "info" | "gray";

export function Pill({ tone, children, spinner }: { tone: Tone; children: ReactNode; spinner?: boolean }) {
  return (
    <span className={`pill ${tone}`}>
      {spinner && <span className="spinner" aria-hidden="true" />}
      {children}
    </span>
  );
}

/** A determinate progress bar. `value` is 0..1. */
export function ProgressBar({ value, label, tone }: { value: number; label: string; tone?: Exclude<Tone, "info" | "gray"> }) {
  const pct = Math.round(Math.min(1, Math.max(0, value)) * 100);
  return (
    <div
      className={`bar${tone ? ` ${tone}` : ""}`}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
    >
      <i style={{ width: `${pct}%` }} />
    </div>
  );
}

/** Tone for a 0..1 score: green from 0.75, amber from 0.5, red below. */
export function scoreTone(score: number): "ok" | "warn" | "bad" {
  return score >= 0.75 ? "ok" : score >= 0.5 ? "warn" : "bad";
}

/** A single-choice segmented control (radio group). */
export function Segmented<T extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const move = (index: number) => {
    const n = options.length;
    const next = options[(index + n) % n];
    if (!next) return;
    onChange(next.value);
    refs.current[(index + n) % n]?.focus();
  };
  return (
    <div className="seg" role="radiogroup" aria-label={label}>
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={(el) => {
            refs.current[i] = el;
          }}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          tabIndex={o.value === value ? 0 : -1}
          onClick={() => onChange(o.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowRight" || e.key === "ArrowDown") {
              e.preventDefault();
              move(i + 1);
            } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
              e.preventDefault();
              move(i - 1);
            }
          }}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** The platform whose conventions this window follows. */
export function usePlatform(): Platform {
  return useMemo(() => detectPlatform(), []);
}

/**
 * A right-aligned row of dialog buttons in the platform's order: the primary button last on
 * macOS, first on Windows. `leading` buttons (such as "Skip this version") stay on the left.
 */
export function ButtonRow({ primary, others = [], leading }: { primary: ReactNode; others?: ReactNode[]; leading?: ReactNode }) {
  const platform = usePlatform();
  const ordered = orderButtons(platform, others, primary);
  return (
    <div className="row-end">
      {leading}
      {leading && <span className="grow" />}
      {ordered.map((b, i) => (
        <span key={i} style={{ display: "contents" }}>
          {b}
        </span>
      ))}
    </div>
  );
}

const FOCUSABLE = 'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea, [tabindex]:not([tabindex="-1"])';

/**
 * A modal dialog: focus moves into it, Tab stays inside it, Escape calls `onEscape`, and focus
 * returns to where it was when the dialog closes.
 */
export function Modal({
  labelledBy,
  describedBy,
  onEscape,
  children,
}: {
  labelledBy: string;
  describedBy?: string;
  onEscape?: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const node = ref.current;
    const preferred = node?.querySelector<HTMLElement>("[data-autofocus]");
    (preferred ?? node?.querySelector<HTMLElement>(FOCUSABLE))?.focus();
    return () => previous?.focus?.();
  }, []);
  return (
    <div className="backdrop">
      <div
        ref={ref}
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        aria-describedby={describedBy}
        onKeyDown={(e) => {
          if (e.key === "Escape" && onEscape) {
            e.stopPropagation();
            onEscape();
          } else if (e.key === "Tab" && ref.current) {
            const items = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
            const first = items[0];
            const last = items.at(-1);
            if (!first || !last) return;
            if (e.shiftKey && document.activeElement === first) {
              e.preventDefault();
              last.focus();
            } else if (!e.shiftKey && document.activeElement === last) {
              e.preventDefault();
              first.focus();
            }
          }
        }}
      >
        {children}
      </div>
    </div>
  );
}

/** The app's icon as drawn in dialogs (matches the placeholder icon until branding lands). */
export function AppIcon({ size = 56 }: { size?: number }) {
  return (
    <svg className="app-icon" width={size} height={size} viewBox="0 0 56 56" aria-hidden="true">
      <defs>
        <linearGradient id="mi-icon" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#2f6fde" />
          <stop offset="1" stopColor="#7a4fd8" />
        </linearGradient>
      </defs>
      <rect width="56" height="56" rx="14" fill="url(#mi-icon)" />
      <path d="M22 17 L40 28 L22 39 Z" fill="#ffffff" />
    </svg>
  );
}

export function FolderIcon() {
  return (
    <svg className="drop-icon" viewBox="0 0 48 48" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinejoin="round">
      <path d="M5 13a3 3 0 0 1 3-3h10l4 4h18a3 3 0 0 1 3 3v19a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3z" />
      <path d="M24 21v11M19 27l5 5 5-5" strokeLinecap="round" />
    </svg>
  );
}
