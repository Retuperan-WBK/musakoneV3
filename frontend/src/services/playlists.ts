import type { Playlist, PlaylistWithTracks } from '../types';
import { getConfigSync } from './config';

function getBackendUrl(): string {
    return getConfigSync().backendHttpUrl;
}

function getAuthHeaders(): Record<string, string> {
    const token = localStorage.getItem('musakone_token');
    return {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
    };
}

async function handleResponse<T>(response: Response): Promise<T> {
    if (!response.ok) {
        const error = await response.json().catch(() => ({ error: `HTTP ${response.status}` }));
        throw new Error(error.error || `HTTP ${response.status}`);
    }
    return response.json();
}

export async function listPlaylists(): Promise<Playlist[]> {
    const response = await fetch(`${getBackendUrl()}/api/playlists`, {
        headers: getAuthHeaders(),
    });
    return handleResponse(response);
}

export async function listPublicPlaylists(): Promise<Playlist[]> {
    const response = await fetch(`${getBackendUrl()}/api/playlists/public`, {
        headers: getAuthHeaders(),
    });
    return handleResponse(response);
}

export async function getPlaylistsContainingTrack(trackUri: string): Promise<number[]> {
    const response = await fetch(
        `${getBackendUrl()}/api/playlists/containing?track_uri=${encodeURIComponent(trackUri)}`,
        { headers: getAuthHeaders() }
    );
    return handleResponse(response);
}

export async function createPlaylist(
    name: string,
    description?: string,
    is_public?: boolean
): Promise<Playlist> {
    const response = await fetch(`${getBackendUrl()}/api/playlists`, {
        method: 'POST',
        headers: getAuthHeaders(),
        body: JSON.stringify({
            name,
            description: description || null,
            is_public: is_public ?? false,
        }),
    });
    return handleResponse(response);
}

export async function getPlaylist(id: number): Promise<PlaylistWithTracks> {
    const response = await fetch(`${getBackendUrl()}/api/playlists/${id}`, {
        headers: getAuthHeaders(),
    });
    return handleResponse(response);
}

export async function updatePlaylist(
    id: number,
    name: string,
    description?: string,
    is_public?: boolean
): Promise<Playlist> {
    const response = await fetch(`${getBackendUrl()}/api/playlists/${id}`, {
        method: 'PUT',
        headers: getAuthHeaders(),
        body: JSON.stringify({
            name,
            description: description || null,
            is_public: is_public ?? false,
        }),
    });
    return handleResponse(response);
}

export async function deletePlaylist(id: number): Promise<void> {
    const response = await fetch(`${getBackendUrl()}/api/playlists/${id}`, {
        method: 'DELETE',
        headers: getAuthHeaders(),
    });
    await handleResponse(response);
}

export async function addTrackToPlaylist(playlistId: number, trackUri: string): Promise<void> {
    const response = await fetch(`${getBackendUrl()}/api/playlists/${playlistId}/tracks`, {
        method: 'POST',
        headers: getAuthHeaders(),
        body: JSON.stringify({ track_uri: trackUri }),
    });
    await handleResponse(response);
}

export async function removeTrackFromPlaylist(playlistId: number, trackUri: string): Promise<void> {
    const response = await fetch(`${getBackendUrl()}/api/playlists/${playlistId}/tracks/remove`, {
        method: 'POST',
        headers: getAuthHeaders(),
        body: JSON.stringify({ track_uri: trackUri }),
    });
    await handleResponse(response);
}

export async function reorderPlaylistTrack(
    playlistId: number,
    trackUri: string,
    newPosition: number
): Promise<void> {
    const response = await fetch(`${getBackendUrl()}/api/playlists/${playlistId}/tracks/reorder`, {
        method: 'PUT',
        headers: getAuthHeaders(),
        body: JSON.stringify({ track_uri: trackUri, new_position: newPosition }),
    });
    await handleResponse(response);
}
