// Queue store
import { atom } from 'nanostores';
import type { QueueTrack } from '../types';

export const queue = atom<QueueTrack[]>([]);
export const scrollToCurrentTrack = atom<number>(0);

export function setQueue(tracks: QueueTrack[]): void {
    queue.set(tracks);
}

export function triggerScrollToCurrent(): void {
    scrollToCurrentTrack.set(Date.now());
}
