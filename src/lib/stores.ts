import { writable } from "svelte/store";
import type { ToastKind, ToastPayload } from "./types";

// ---- Toasts -------------------------------------------------------------

export interface Toast extends ToastPayload {
  id: number;
}

function createToasts() {
  const { subscribe, update } = writable<Toast[]>([]);
  let seq = 0;

  function push(kind: ToastKind, message: string, ttl = 4000) {
    const id = ++seq;
    update((list) => [...list, { id, kind, message }]);
    setTimeout(() => dismiss(id), ttl);
  }

  function dismiss(id: number) {
    update((list) => list.filter((t) => t.id !== id));
  }

  return {
    subscribe,
    dismiss,
    info: (m: string) => push("info", m),
    success: (m: string) => push("success", m),
    error: (m: string) => push("error", m),
    fromPayload: (p: ToastPayload) => push(p.kind, p.message),
  };
}

export const toasts = createToasts();

// ---- App uptime (drives the uptime counter cheaply on the client) -------

export const appStart = Date.now();
