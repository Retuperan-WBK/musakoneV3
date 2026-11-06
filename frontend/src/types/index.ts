// Core types for MusakoneV3

export interface Track {
    uri: string;
    name: string;
    artists?: Artist[];
    album?: Album;
    duration: number;
}

export interface Artist {
    uri: string;
    name: string;
}

export interface Album {
    uri: string;
    name: string;
    artists?: Artist[];
}

export interface PlaybackState {
    state: 'playing' | 'paused' | 'stopped';
    currentTrack: Track | null;
    timePosition: number;
    volume: number;
}

export interface QueueTrack {
    tlid: number;
    track: Track;
}

// Playlist types
export interface Playlist {
    id: number;
    user_id: number;
    /** Owner's username; every playlist is visible to the whole room */
    username: string;
    name: string;
    description: string | null;
    created_at: number;
    updated_at: number;
}

export interface PlaylistTrack {
    playlist_id: number;
    track_uri: string;
    position: number;
}

export interface PlaylistWithTracks {
    playlist: Playlist;
    tracks: PlaylistTrack[];
}
