/**
 * Library browsing state management
 */

import { atom, computed } from 'nanostores';
import { cachedBrowse, libraryRevision } from '../services/library-cache';
import type { LibraryRef } from '../services/mopidy';
import { browse } from '../services/mopidy';

export interface BreadcrumbItem {
    uri: string | null;
    name: string;
    scrollTop?: number;
}

// Current library items
export const libraryItems = atom<LibraryRef[]>([]);

// Navigation path (breadcrumbs)
export const libraryPath = atom<BreadcrumbItem[]>([{ uri: null, name: 'Library' }]);

// Loading state
export const libraryLoading = atom<boolean>(false);

// Error state
export const libraryError = atom<string | null>(null);

// Current URI being browsed
export const currentUri = computed(libraryPath, (path) => {
    return path[path.length - 1]?.uri ?? null;
});

let navigation = 0;
let active: { uri: string | null; revision: number; promise: Promise<void> } | undefined;

export function loadLibrary(): Promise<void> {
    const uri = currentUri.get();
    const revision = libraryRevision.get();
    if (active?.uri === uri && active.revision === revision) return active.promise;
    const version = ++navigation;
    const cached = cachedBrowse(uri);
    libraryItems.set(cached ?? []);
    libraryError.set(null);
    libraryLoading.set(!cached);
    if (cached) {
        active = undefined;
        return Promise.resolve();
    }
    const promise = browse(uri ?? undefined)
        .then((items) => {
            if (version === navigation && revision === libraryRevision.get())
                libraryItems.set(items);
        })
        .catch((error: unknown) => {
            if (version === navigation && revision === libraryRevision.get()) {
                libraryError.set(error instanceof Error ? error.message : 'Failed to load library');
            }
        })
        .finally(() => {
            if (version === navigation) {
                libraryLoading.set(false);
                active = undefined;
            }
        });
    active = { uri, revision, promise };
    return promise;
}

function changePath(path: BreadcrumbItem[]): void {
    navigation++;
    active = undefined;
    libraryPath.set(path);
    void loadLibrary();
}

export function saveLibraryScroll(scrollTop: number): void {
    // Scroll bookkeeping doesn't need to re-render breadcrumbs on every scroll event.
    const current = libraryPath.get().at(-1);
    if (current) current.scrollTop = scrollTop;
}

export function navigateTo(uri: string | null, name: string): void {
    const path = libraryPath.get();
    changePath([...path, { uri, name }]);
}

export function navigateBack(): void {
    const path = libraryPath.get();
    if (path.length > 1) {
        changePath(path.slice(0, -1));
    }
}

export function navigateToIndex(index: number): void {
    const path = libraryPath.get();
    if (index >= 0 && index < path.length) {
        changePath(path.slice(0, index + 1));
    }
}

export function resetLibrary(): void {
    navigation++;
    active = undefined;
    libraryItems.set([]);
    libraryPath.set([{ uri: null, name: 'Library' }]);
    libraryError.set(null);
    libraryLoading.set(false);
}
