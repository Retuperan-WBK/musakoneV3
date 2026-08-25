import { useStore } from '@nanostores/preact';
import { atom } from 'nanostores';
import { useModalDismiss } from '../hooks/useModalDismiss';

interface ConfirmState {
    open: boolean;
    title: string;
    message: string;
    confirmLabel: string;
    destructive: boolean;
    onConfirm: (() => void) | null;
}

const confirmState = atom<ConfirmState>({
    open: false,
    title: '',
    message: '',
    confirmLabel: 'Confirm',
    destructive: false,
    onConfirm: null,
});

export function confirm(opts: {
    title: string;
    message: string;
    confirmLabel?: string;
    destructive?: boolean;
    onConfirm: () => void;
}): void {
    confirmState.set({
        open: true,
        title: opts.title,
        message: opts.message,
        confirmLabel: opts.confirmLabel ?? 'Confirm',
        destructive: opts.destructive ?? false,
        onConfirm: opts.onConfirm,
    });
}

function close(): void {
    confirmState.set({ ...confirmState.get(), open: false, onConfirm: null });
}

/** Gate: mounts the dialog only while open so its Escape/Back handling is scoped to that time */
export function ConfirmModal() {
    const state = useStore(confirmState);
    if (!state.open) return null;
    return <ConfirmDialog state={state} />;
}

function ConfirmDialog({ state }: { state: ConfirmState }) {
    useModalDismiss(close);

    return (
        // biome-ignore lint/a11y/noStaticElementInteractions: backdrop; Escape/Back handled by useModalDismiss
        // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop; Escape/Back handled by useModalDismiss
        <div
            className="fixed inset-0 z-200 flex items-center justify-center bg-black/60 px-6"
            onClick={(e) => {
                if (e.target === e.currentTarget) close();
            }}
        >
            <div
                className="w-full max-w-sm bg-bg-secondary border border-border-primary flex flex-col"
                role="alertdialog"
                aria-modal="true"
                aria-labelledby="confirm-title"
            >
                <div className="px-4 pt-4 pb-2">
                    <div id="confirm-title" className="text-fg-primary text-sm font-medium">
                        {state.title}
                    </div>
                    <div className="text-fg-secondary text-sm mt-1">{state.message}</div>
                </div>
                <div className="flex gap-2 px-4 pb-4 pt-2">
                    <button
                        type="button"
                        className="flex-1 min-h-11 px-3 py-2 bg-transparent border border-border-primary text-fg-secondary font-mono text-sm cursor-pointer transition-all duration-150 hover:text-fg-primary hover:border-fg-secondary"
                        onClick={close}
                    >
                        Cancel
                    </button>
                    <button
                        type="button"
                        className={`flex-1 min-h-11 px-3 py-2 border-none font-mono text-sm cursor-pointer transition-all duration-150 ${
                            state.destructive
                                ? 'bg-error text-fg-primary hover:opacity-80'
                                : 'bg-accent-primary text-fg-primary hover:opacity-80'
                        }`}
                        onClick={() => {
                            state.onConfirm?.();
                            close();
                        }}
                    >
                        {state.confirmLabel}
                    </button>
                </div>
            </div>
        </div>
    );
}
