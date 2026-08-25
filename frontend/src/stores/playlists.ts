import { atom } from 'nanostores';
import type { Playlist, PlaylistTrack } from '../types';

// List view state
export const playlists = atom<Playlist[]>([]);
export const publicPlaylists = atom<Playlist[]>([]);
export const playlistsLoading = atom(false);
export const playlistsError = atom<string | null>(null);

// Detail view state
export const currentPlaylist = atom<Playlist | null>(null);
export const currentPlaylistTracks = atom<PlaylistTrack[]>([]);

// Add-to-playlist modal state
export const addToPlaylistModalOpen = atom(false);
export const addToPlaylistTrackUri = atom<string | null>(null);

export function setPlaylists(data: Playlist[]): void {
    playlists.set(data);
}

export function setPublicPlaylists(data: Playlist[]): void {
    publicPlaylists.set(data);
}

export function setPlaylistsLoading(loading: boolean): void {
    playlistsLoading.set(loading);
}

export function setPlaylistsError(error: string | null): void {
    playlistsError.set(error);
}

export function setCurrentPlaylist(playlist: Playlist | null): void {
    currentPlaylist.set(playlist);
}

export function setCurrentPlaylistTracks(tracks: PlaylistTrack[]): void {
    currentPlaylistTracks.set(tracks);
}

export function openAddToPlaylistModal(trackUri: string): void {
    addToPlaylistTrackUri.set(trackUri);
    addToPlaylistModalOpen.set(true);
}

export function closeAddToPlaylistModal(): void {
    addToPlaylistModalOpen.set(false);
    addToPlaylistTrackUri.set(null);
}
