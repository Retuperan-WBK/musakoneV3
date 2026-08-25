/**
 * Library browsing state management
 */

import { atom, computed } from 'nanostores';
import type { LibraryRef } from '../services/mopidy';

export interface BreadcrumbItem {
    uri: string | null;
    name: string;
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

// Actions
export function setLibraryItems(items: LibraryRef[]): void {
    libraryItems.set(items);
    libraryError.set(null);
}

export function setLibraryLoading(loading: boolean): void {
    libraryLoading.set(loading);
}

export function setLibraryError(error: string | null): void {
    libraryError.set(error);
}

export function navigateTo(uri: string | null, name: string): void {
    const path = libraryPath.get();
    libraryPath.set([...path, { uri, name }]);
}

export function navigateBack(): void {
    const path = libraryPath.get();
    if (path.length > 1) {
        libraryPath.set(path.slice(0, -1));
    }
}

export function navigateToIndex(index: number): void {
    const path = libraryPath.get();
    if (index >= 0 && index < path.length) {
        libraryPath.set(path.slice(0, index + 1));
    }
}

export function resetLibrary(): void {
    libraryItems.set([]);
    libraryPath.set([{ uri: null, name: 'Library' }]);
    libraryError.set(null);
}
