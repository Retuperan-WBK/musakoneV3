import { useStore } from '@nanostores/preact';
import { dismissToast, toasts } from '../stores/toast';

const KIND_CLASS = {
    error: 'border-error text-error',
    success: 'border-success text-success',
    info: 'border-border-primary text-fg-primary',
} as const;

/** Stack of transient messages, tap to dismiss. Rendered once in Layout. */
export function Toast() {
    const items = useStore(toasts);
    if (items.length === 0) return null;

    return (
        <output
            className="fixed left-0 right-0 top-12 z-300 flex flex-col items-center gap-1 px-4 pointer-events-none"
            aria-live="polite"
        >
            {items.map((toast) => (
                <button
                    type="button"
                    key={toast.id}
                    className={`pointer-events-auto max-w-full truncate px-3 py-2 bg-bg-tertiary border font-mono text-sm shadow-lg cursor-pointer ${KIND_CLASS[toast.kind]}`}
                    onClick={() => dismissToast(toast.id)}
                >
                    {toast.text}
                </button>
            ))}
        </output>
    );
}
