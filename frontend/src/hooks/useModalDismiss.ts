import { useEffect, useRef } from 'preact/hooks';

interface ModalHistoryState {
    musakoneModal: true;
}

function isModalState(state: unknown): state is ModalHistoryState {
    return typeof state === 'object' && state !== null && 'musakoneModal' in state;
}

/**
 * Close a modal on Escape and on the browser/Android Back button.
 *
 * A history entry is pushed while the modal is open so that Back closes the modal
 * instead of navigating the page underneath it. If the modal is closed any other way,
 * the entry is consumed again so Back keeps working normally afterwards.
 */
export function useModalDismiss(onClose: () => void): void {
    const closeRef = useRef(onClose);
    closeRef.current = onClose;

    useEffect(() => {
        const state: ModalHistoryState = { musakoneModal: true };
        window.history.pushState(state, '', window.location.href);
        let closedByHistory = false;

        const handlePopState = () => {
            closedByHistory = true;
            closeRef.current();
        };
        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                e.preventDefault();
                closeRef.current();
            }
        };

        window.addEventListener('popstate', handlePopState);
        window.addEventListener('keydown', handleKeyDown);

        return () => {
            window.removeEventListener('popstate', handlePopState);
            window.removeEventListener('keydown', handleKeyDown);
            if (!closedByHistory && isModalState(window.history.state)) {
                window.history.back();
            }
        };
    }, []);
}
