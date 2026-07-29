/** Session resume helpers for Web Chat Phase 4b (VL-UI-007). */

import type { SessionSummary } from "./api";

export const ACTIVE_SESSION_KEY = "velaclaw_active_session";
export const SESSION_URL_PARAM = "session";

export function loadActiveSessionId(): string | null {
  try {
    const id = localStorage.getItem(ACTIVE_SESSION_KEY);
    return id && id.trim() ? id.trim() : null;
  } catch {
    return null;
  }
}

export function saveActiveSessionId(id: string | null): void {
  try {
    if (id) {
      localStorage.setItem(ACTIVE_SESSION_KEY, id);
    } else {
      localStorage.removeItem(ACTIVE_SESSION_KEY);
    }
  } catch {
    /* ignore */
  }
}

/** Pick session id from URL ?session= or localStorage fallback. */
export function resolveInitialSessionId(search: string): string | null {
  const params = new URLSearchParams(search);
  const fromUrl = params.get(SESSION_URL_PARAM)?.trim();
  if (fromUrl) return fromUrl;
  return loadActiveSessionId();
}

/** Update ?session= in the address bar without navigation. */
export function syncSessionUrl(id: string | null): void {
  const url = new URL(window.location.href);
  if (id) {
    url.searchParams.set(SESSION_URL_PARAM, id);
  } else {
    url.searchParams.delete(SESSION_URL_PARAM);
  }
  window.history.replaceState({}, "", `${url.pathname}${url.search}`);
}

export function formatRelativeTime(iso: string): string {
  const parsed = Date.parse(iso);
  if (Number.isNaN(parsed)) return iso;
  const deltaMs = Date.now() - parsed;
  const sec = Math.floor(deltaMs / 1000);
  if (sec < 60) return "just now";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 48) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  return `${day}d ago`;
}

export function formatSessionMeta(session: SessionSummary): string {
  const parts = [`${session.message_count} msgs`, formatRelativeTime(session.updated_at)];
  if (session.model_id) {
    parts.push(session.model_id);
  }
  return parts.join(" · ");
}
