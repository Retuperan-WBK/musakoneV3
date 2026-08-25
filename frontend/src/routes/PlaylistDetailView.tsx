import { useStore } from '@nanostores/preact';
import { ChevronLeft, Edit3, Music, Play, Trash2, X } from 'lucide-preact';
import { useEffect, useMemo, useState } from 'preact/hooks';
import { useLocation, useRoute } from 'wouter';
import { TrackItem } from '../components/TrackItem';
import { useAddToQueue } from '../hooks/useAddToQueue';
import * as mopidy from '../services/mopidy';
import * as playlistService from '../services/playlists';
import {
    currentPlaylist,
    currentPlaylistTracks,
    setCurrentPlaylist,
    setCurrentPlaylistTracks,
} from '../stores/playlists';
import { queue } from '../stores/queue';
import { toastError } from '../stores/toast';
import type { Track } from '../types';

export function PlaylistDetailView() {
    const [, params] = useRoute('/playlists/:id');
    const [, setLocation] = useLocation();
    const playlist = useStore(currentPlaylist);
    const tracks = useStore(currentPlaylistTracks);
    const queueTracks = useStore(queue);
    const { addToQueue } = useAddToQueue();
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [editing, setEditing] = useState(false);
    const [editName, setEditName] = useState('');
    const [editDesc, setEditDesc] = useState('');
    const [trackInfo, setTrackInfo] = useState<Map<string, Track>>(new Map());

    const playlistId = params?.id ? parseInt(params.id, 10) : null;

    const queuedUris = useMemo(() => {
        return new Set(queueTracks.map((t) => t.track.uri));
    }, [queueTracks]);

    const loadPlaylist = async () => {
        if (!playlistId) return;
        setLoading(true);
        setError(null);
        try {
            const data = await playlistService.getPlaylist(playlistId);
            setCurrentPlaylist(data.playlist);
            setCurrentPlaylistTracks(data.tracks);

            // Lookup track metadata from Mopidy
            if (data.tracks.length > 0) {
                const uris = data.tracks.map((t) => t.track_uri);
                try {
                    const lookupResult = await mopidy.lookup(uris);
                    const infoMap = new Map<string, Track>();
                    for (const [uri, trackList] of lookupResult) {
                        const first = trackList[0];
                        if (first) {
                            infoMap.set(uri, first);
                        }
                    }
                    setTrackInfo(infoMap);
                } catch {
                    // Lookup failed (e.g. not connected) — show URIs as fallback
                }
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : 'Failed to load playlist');
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        loadPlaylist();
        return () => {
            setCurrentPlaylist(null);
            setCurrentPlaylistTracks([]);
            setTrackInfo(new Map());
        };
    }, [playlistId]);

    const handlePlayAll = async () => {
        if (tracks.length === 0) return;
        try {
            await addToQueue(tracks.map((t) => t.track_uri));
        } catch (err) {
            console.error('Failed to queue playlist tracks:', err);
            toastError('Could not add playlist to queue');
        }
    };

    const handleRemoveTrack = async (trackUri: string) => {
        if (!playlistId) return;
        try {
            await playlistService.removeTrackFromPlaylist(playlistId, trackUri);
            await loadPlaylist();
        } catch (err) {
            console.error('Failed to remove track:', err);
            toastError('Could not remove track');
        }
    };

    const handleEdit = () => {
        if (!playlist) return;
        setEditName(playlist.name);
        setEditDesc(playlist.description || '');
        setEditing(true);
    };

    const handleSaveEdit = async () => {
        if (!playlistId || !editName.trim()) return;
        try {
            await playlistService.updatePlaylist(
                playlistId,
                editName.trim(),
                editDesc.trim() || undefined
            );
            setEditing(false);
            await loadPlaylist();
        } catch (err) {
            console.error('Failed to update playlist:', err);
            toastError('Could not save playlist');
        }
    };

    const getTrackDisplay = (
        trackUri: string
    ): { name: string; artists?: Array<{ name: string }>; duration?: number } => {
        const info = trackInfo.get(trackUri);
        if (info) {
            return {
                name: info.name,
                artists: info.artists,
                duration: info.duration,
            };
        }
        // Fallback: extract last segment of URI
        return { name: trackUri.split(':').pop() || trackUri };
    };

    if (loading) {
        return (
            <div className="flex items-center justify-center min-h-[50vh] text-fg-secondary">
                Loading...
            </div>
        );
    }

    if (error) {
        return (
            <div className="flex flex-col items-center justify-center min-h-[50vh] gap-4 text-error text-center px-8">
                <p>{error}</p>
                <button
                    type="button"
                    className="px-4 py-2 bg-bg-tertiary border border-border-primary text-fg-primary font-mono text-sm cursor-pointer transition-all duration-150 hover:text-accent-primary hover:border-accent-primary"
                    onClick={loadPlaylist}
                >
                    Retry
                </button>
            </div>
        );
    }

    if (!playlist) {
        return (
            <div className="flex items-center justify-center min-h-[50vh] text-fg-secondary">
                Playlist not found
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Header */}
            <div className="flex items-center gap-2 px-3 py-2 border-b border-border-primary shrink-0 bg-bg-secondary">
                <button
                    type="button"
                    className="flex items-center justify-center w-8 h-8 bg-transparent border-none text-fg-secondary cursor-pointer transition-colors duration-150 hover:text-accent-primary"
                    onClick={() => setLocation('/playlists')}
                    aria-label="Back to playlists"
                >
                    <ChevronLeft size={20} />
                </button>

                {editing ? (
                    <div className="flex-1 flex items-center gap-2">
                        <input
                            type="text"
                            value={editName}
                            onInput={(e) => setEditName((e.target as HTMLInputElement).value)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter') handleSaveEdit();
                                if (e.key === 'Escape') setEditing(false);
                            }}
                            className="flex-1 bg-bg-primary border border-border-primary text-fg-primary font-mono text-sm px-2 py-1 outline-none focus:border-accent-primary"
                            // biome-ignore lint: autofocus intentional
                            autoFocus
                        />
                        <button
                            type="button"
                            className="px-2 py-1 bg-accent-primary text-fg-primary font-mono text-xs border-none cursor-pointer"
                            onClick={handleSaveEdit}
                        >
                            Save
                        </button>
                        <button
                            type="button"
                            className="flex items-center justify-center w-6 h-6 bg-transparent border-none text-fg-tertiary cursor-pointer"
                            onClick={() => setEditing(false)}
                        >
                            <X size={14} />
                        </button>
                    </div>
                ) : (
                    <>
                        <div className="flex-1 min-w-0">
                            <div className="text-fg-primary truncate text-sm font-medium">
                                {playlist.name}
                            </div>
                            {playlist.description && (
                                <div className="text-xs text-fg-tertiary truncate">
                                    {playlist.description}
                                </div>
                            )}
                        </div>
                        <button
                            type="button"
                            className="flex items-center justify-center w-8 h-8 bg-transparent border border-border-primary text-fg-tertiary cursor-pointer shrink-0 transition-all duration-150 hover:text-accent-primary hover:border-accent-primary"
                            onClick={handleEdit}
                            aria-label="Edit playlist"
                        >
                            <Edit3 size={14} />
                        </button>
                    </>
                )}
            </div>

            {/* Actions bar */}
            {tracks.length > 0 && (
                <div className="flex items-center gap-2 px-4 py-2 border-b border-border-secondary bg-bg-secondary">
                    <button
                        type="button"
                        className="flex items-center gap-1 px-3 py-1 bg-transparent border border-border-primary text-fg-secondary text-sm font-mono cursor-pointer transition-all duration-150 hover:text-accent-primary hover:border-accent-primary"
                        onClick={handlePlayAll}
                    >
                        <Play size={14} />
                        Queue All ({tracks.length})
                    </button>
                </div>
            )}

            {/* Track list */}
            {tracks.length === 0 ? (
                <div className="flex flex-col items-center justify-center min-h-[30vh] gap-2 text-fg-secondary text-center px-8">
                    <Music size={24} className="text-fg-tertiary" />
                    <p className="text-sm">No tracks yet</p>
                    <p className="text-xs text-fg-tertiary">Add tracks from Library or Search</p>
                </div>
            ) : (
                <div className="flex-1 overflow-y-auto overscroll-y-contain pb-2">
                    {tracks.map((pt, index) => {
                        const display = getTrackDisplay(pt.track_uri);
                        return (
                            <TrackItem
                                key={pt.track_uri}
                                track={display}
                                icon={<Music size={20} />}
                                showDuration={!!display.duration}
                                customMeta={
                                    queuedUris.has(pt.track_uri)
                                        ? `#${index + 1} · in queue`
                                        : undefined
                                }
                                rightContent={
                                    <button
                                        type="button"
                                        className="flex items-center justify-center w-8 h-8 bg-transparent border border-border-primary text-fg-tertiary cursor-pointer shrink-0 transition-all duration-150 hover:text-error hover:border-error"
                                        onClick={() => handleRemoveTrack(pt.track_uri)}
                                        aria-label="Remove from playlist"
                                    >
                                        <Trash2 size={14} />
                                    </button>
                                }
                            />
                        );
                    })}
                </div>
            )}
        </div>
    );
}
