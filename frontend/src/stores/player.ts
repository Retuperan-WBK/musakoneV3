// Nanostores for state management
import { atom, computed } from 'nanostores';
import type { PlaybackState, Track } from '../types';

// Playback State
export const playbackState = atom<'playing' | 'paused' | 'stopped'>('stopped');
export const currentTrack = atom<Track | null>(null);
export const timePosition = atom<number>(0);
export const volume = atom<number>(80);

// Computed values
export const isPlaying = computed(playbackState, (state) => state === 'playing');

// Actions
export function updatePlaybackState(state: Partial<PlaybackState>): void {
    if (state.state !== undefined) playbackState.set(state.state);
    if (state.currentTrack !== undefined) currentTrack.set(state.currentTrack);
    if (state.timePosition !== undefined) timePosition.set(state.timePosition);
    if (state.volume !== undefined) volume.set(state.volume);
}
