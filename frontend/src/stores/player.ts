// Nanostores for state management
import { atom, computed } from 'nanostores';
import type { PlaybackOptions } from '../services/mopidy';
import type { PlaybackState, Track } from '../types';

// Playback State
export const playbackState = atom<'playing' | 'paused' | 'stopped'>('stopped');
export const currentTrack = atom<Track | null>(null);
export const timePosition = atom<number>(0);
export const volume = atom<number>(80);
export const repeat = atom<'off' | 'track' | 'all'>('off');
export const random = atom<boolean>(false);

// Tracklist options (repeat / random / single / consume); null until first loaded
export const playbackOptions = atom<PlaybackOptions | null>(null);

// Computed values
export const isPlaying = computed(playbackState, (state) => state === 'playing');

export function setPlaybackOptions(options: PlaybackOptions): void {
    playbackOptions.set(options);
}

// Helper function to get full playback state
export function getPlaybackState(): PlaybackState {
    return {
        state: playbackState.get(),
        currentTrack: currentTrack.get(),
        timePosition: timePosition.get(),
        volume: volume.get(),
        repeat: repeat.get(),
        random: random.get(),
    };
}

// Actions
export function updatePlaybackState(state: Partial<PlaybackState>): void {
    if (state.state !== undefined) playbackState.set(state.state);
    if (state.currentTrack !== undefined) currentTrack.set(state.currentTrack);
    if (state.timePosition !== undefined) timePosition.set(state.timePosition);
    if (state.volume !== undefined) volume.set(state.volume);
    if (state.repeat !== undefined) repeat.set(state.repeat);
    if (state.random !== undefined) random.set(state.random);
}
