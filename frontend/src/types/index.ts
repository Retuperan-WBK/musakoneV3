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
    repeat: 'off' | 'track' | 'all';
    random: boolean;
}

export interface QueueTrack {
    tlid: number;
    track: Track;
}

export interface WebSocketMessage {
    type: string;
    data?: unknown;
}

export interface BackendMessage {
    event?: string;
    method?: string;
    params?: unknown;
    result?: unknown;
    error?: {
        message: string;
        code?: number;
    };
}

// Analytics types
export interface PlaybackEvent {
    id: number;
    user_id: number;
    timestamp_ms: number;
    event_type: string;
    track_uri?: string;
    track_name?: string;
    artist_name?: string;
    album_name?: string;
    track_duration_ms?: number;
    position_ms?: number;
    seek_to_ms?: number;
    volume_level?: number;
    playback_flags?: string;
}

export interface QueueEvent {
    id: number;
    user_id: number;
    timestamp_ms: number;
    event_type: string;
    track_uris?: string;
    track_names?: string;
    at_position?: number;
    from_position?: number;
    to_position?: number;
    queue_length?: number;
}

export interface SearchEvent {
    id: number;
    user_id: number;
    timestamp_ms: number;
    event_type: string;
    query_text?: string;
    browse_uri?: string;
    result_count?: number;
}

export interface AnalyticsData {
    counts: {
        playback_events: number;
        queue_events: number;
        search_events: number;
    };
    offset: number;
    limit: number;
    playback: PlaybackEvent[];
    queue: QueueEvent[];
    search: SearchEvent[];
}

export interface UserStats {
    [actionType: string]: number;
}

// Playlist types
export interface Playlist {
    id: number;
    user_id: number;
    name: string;
    description: string | null;
    is_public: boolean;
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
