/**
 * Transient user-facing notifications (errors and confirmations).
 * Anything that can fail from a tap must report through here – a silent
 * console.error looks like "the app is broken" to someone holding a phone.
 */
import { atom } from 'nanostores';

export type ToastKind = 'error' | 'success' | 'info';

export interface ToastMessage {
    id: number;
    text: string;
    kind: ToastKind;
}

export const toasts = atom<ToastMessage[]>([]);

let nextId = 1;

export function showToast(text: string, kind: ToastKind = 'info', durationMs = 3000): void {
    const id = nextId++;
    // Keep at most three on screen; the oldest is dropped
    toasts.set([...toasts.get().slice(-2), { id, text, kind }]);
    window.setTimeout(() => dismissToast(id), durationMs);
}

export function dismissToast(id: number): void {
    toasts.set(toasts.get().filter((t) => t.id !== id));
}

export function toastError(text: string): void {
    showToast(text, 'error', 4000);
}
