/**
 * Authentication state management
 */

import { atom } from 'nanostores';
import type { User } from '../services/auth';

/**
 * Current authenticated user (null if not logged in)
 */
export const currentUser = atom<User | null>(null);

/**
 * Authentication loading state
 */
export const authLoading = atom<boolean>(false);

/**
 * Authentication error message
 */
export const authError = atom<string | null>(null);

/**
 * Set current user
 */
export function setUser(user: User | null): void {
    currentUser.set(user);
}

/**
 * Set loading state
 */
export function setAuthLoading(loading: boolean): void {
    authLoading.set(loading);
}

/**
 * Set error message
 */
export function setAuthError(error: string | null): void {
    authError.set(error);
}

/**
 * Clear all auth state
 */
export function clearAuth(): void {
    currentUser.set(null);
    authError.set(null);
    authLoading.set(false);
}
