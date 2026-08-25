/**
 * Mix API: recommendations, autoplay control and listening insights.
 */
import { getToken } from './auth';
import { getConfigSync } from './config';

export interface Recommendation {
    uri: string;
    name: string;
    artist: string;
    album: string | null;
    duration_ms: number | null;
    score: number;
    reasons: string[];
}

export type PicksScope = 'me' | 'room';

export interface AutoplaySettings {
    enabled: boolean;
    min_ahead: number;
    batch_size: number;
    discovery: number;
    mode: string;
}

export interface AutoplayStatus {
    settings: AutoplaySettings;
    added_24h: number;
}

export interface NowPlaying {
    playback_state: string | null;
    track_uri: string | null;
    track_name: string | null;
    artist_name: string | null;
    album_name: string | null;
    track_duration_ms: number | null;
    position_ms: number | null;
    volume: number | null;
    queue_length: number;
}

export interface InsightTrack {
    uri: string;
    name: string | null;
    artist: string | null;
    plays: number;
    score: number;
    users?: number;
    my_plays?: number;
}

export interface InsightArtist {
    name: string;
    score: number;
    users?: number;
    plays: number;
}

export interface Insights {
    generated_at: number;
    now_playing: NowPlaying;
    autoplay: AutoplayStatus;
    room: {
        totals: {
            plays: number;
            plays_24h: number;
            listen_minutes: number;
            queue_adds: number;
            autoplay_adds_24h: number;
            users: number;
        };
        top_tracks: InsightTrack[];
        top_artists: InsightArtist[];
        taste_map: Array<{
            user_id: number;
            username: string;
            artists: Array<{ name: string; score: number }>;
        }>;
        hourly: Array<{ hour: number; plays: number }>;
        daily: Array<{ day: string; plays: number }>;
        active_users: Array<{ id: number; username: string; last_activity: number }>;
        recent_adds: Array<{
            timestamp_ms: number;
            username: string;
            tracks: string[];
            autoplay: boolean;
        }>;
        recent_plays: Array<{
            timestamp_ms: number;
            uri: string | null;
            name: string | null;
            artist: string | null;
            username: string | null;
        }>;
    };
    me: {
        user_id: number;
        top_artists: InsightArtist[];
        top_tracks: InsightTrack[];
        stats: Record<string, number>;
    };
}

async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
    const token = getToken();
    const response = await fetch(`${getConfigSync().backendHttpUrl}${path}`, {
        ...init,
        headers: {
            'Content-Type': 'application/json',
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
            ...(init.headers ?? {}),
        },
    });
    if (!response.ok) {
        const body = await response.json().catch(() => ({}));
        throw new Error(typeof body.error === 'string' ? body.error : `HTTP ${response.status}`);
    }
    return response.json() as Promise<T>;
}

export function getRecommendations(
    scope: PicksScope,
    limit = 20
): Promise<{ items: Recommendation[] }> {
    return api(`/api/recommendations?scope=${scope}&limit=${limit}`);
}

export function getAutoplay(): Promise<AutoplayStatus> {
    return api('/api/autoplay');
}

export function updateAutoplay(patch: Partial<AutoplaySettings>): Promise<AutoplayStatus> {
    return api('/api/autoplay', { method: 'PUT', body: JSON.stringify(patch) });
}

export function fillAutoplay(): Promise<{ added: number }> {
    return api('/api/autoplay/fill', { method: 'POST' });
}

export function getInsights(): Promise<Insights> {
    return api('/api/insights');
}
