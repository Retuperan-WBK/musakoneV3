import { useStore } from '@nanostores/preact';
import { Check, ListMusic, Plus, X } from 'lucide-preact';
import { useEffect, useState } from 'preact/hooks';
import { useModalDismiss } from '../hooks/useModalDismiss';
import * as playlistService from '../services/playlists';
import {
    addToPlaylistModalOpen,
    addToPlaylistTrackUri,
    closeAddToPlaylistModal,
    playlists,
    setPlaylists,
} from '../stores/playlists';
import { showToast, toastError } from '../stores/toast';
import type { Playlist } from '../types';

/**
 * Gate component: reads store, only renders the inner modal when open.
 * This guarantees AddToPlaylistModalInner unmounts on close and
 * remounts fresh on every open — no stale state.
 */
export function AddToPlaylistModal() {
    const isOpen = useStore(addToPlaylistModalOpen);
    const trackUri = useStore(addToPlaylistTrackUri);

    if (!isOpen || !trackUri) return null;

    return <AddToPlaylistModalInner trackUri={trackUri} />;
}

function AddToPlaylistModalInner({ trackUri }: { trackUri: string }) {
    const items = useStore(playlists);
    const [creating, setCreating] = useState(false);
    const [newName, setNewName] = useState('');
    const [loading, setLoading] = useState(false);
    const [containingIds, setContainingIds] = useState<Set<number>>(new Set());

    useModalDismiss(closeAddToPlaylistModal);

    useEffect(() => {
        let cancelled = false;

        playlistService.listPlaylists().then(setPlaylists).catch(console.error);
        playlistService
            .getPlaylistsContainingTrack(trackUri)
            .then((ids) => {
                if (!cancelled) setContainingIds(new Set(ids));
            })
            .catch(console.error);

        return () => {
            cancelled = true;
        };
    }, [trackUri]);

    const handleAdd = async (playlist: Playlist) => {
        if (containingIds.has(playlist.id)) return;
        setLoading(true);
        try {
            await playlistService.addTrackToPlaylist(playlist.id, trackUri);
            setContainingIds(new Set([...containingIds, playlist.id]));
            showToast(`Added to ${playlist.name}`, 'success');
        } catch (err) {
            console.error('Failed to add track to playlist:', err);
            toastError(`Could not add to ${playlist.name}`);
        } finally {
            setLoading(false);
        }
    };

    const handleCreate = async () => {
        const name = newName.trim();
        if (!name) return;
        setLoading(true);
        try {
            const playlist = await playlistService.createPlaylist(name);
            await playlistService.addTrackToPlaylist(playlist.id, trackUri);
            setContainingIds(new Set([...containingIds, playlist.id]));
            const updated = await playlistService.listPlaylists();
            setPlaylists(updated);
            showToast(`Created ${name}`, 'success');
            setNewName('');
            setCreating(false);
        } catch (err) {
            console.error('Failed to create playlist:', err);
            toastError(`Could not create playlist ${name}`);
        } finally {
            setLoading(false);
        }
    };

    return (
        // biome-ignore lint/a11y/noStaticElementInteractions: backdrop; Escape/Back handled by useModalDismiss
        // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop; Escape/Back handled by useModalDismiss
        <div
            className="fixed inset-0 z-200 flex items-end justify-center bg-black/60"
            onClick={(e) => {
                if (e.target === e.currentTarget) closeAddToPlaylistModal();
            }}
        >
            <div className="w-full max-w-lg bg-bg-secondary border-t border-border-primary max-h-[60vh] flex flex-col">
                {/* Header */}
                <div className="flex items-center justify-between px-4 py-3 border-b border-border-primary shrink-0">
                    <span className="text-fg-primary text-sm font-medium">Add to Playlist</span>
                    <button
                        type="button"
                        className="flex items-center justify-center w-8 h-8 bg-transparent border-none text-fg-tertiary cursor-pointer transition-colors duration-150 hover:text-fg-primary"
                        onClick={closeAddToPlaylistModal}
                    >
                        <X size={18} />
                    </button>
                </div>

                {/* Playlist list */}
                <div className="flex-1 overflow-y-auto">
                    {items.map((playlist) => {
                        const alreadyAdded = containingIds.has(playlist.id);
                        return (
                            <button
                                type="button"
                                key={playlist.id}
                                className={`flex items-center gap-3 w-full px-4 py-3 bg-transparent border-none border-b-2 border-border-secondary text-left transition-all duration-150 ${alreadyAdded ? 'opacity-60 cursor-default' : 'cursor-pointer hover:bg-bg-tertiary'} ${loading ? 'opacity-50 pointer-events-none' : ''}`}
                                onClick={() => handleAdd(playlist)}
                                disabled={alreadyAdded}
                            >
                                <ListMusic
                                    size={18}
                                    className={`shrink-0 ${alreadyAdded ? 'text-success' : 'text-fg-tertiary'}`}
                                />
                                <span className="text-fg-primary text-sm truncate flex-1">
                                    {playlist.name}
                                </span>
                                {alreadyAdded && (
                                    <Check size={16} className="text-success shrink-0" />
                                )}
                            </button>
                        );
                    })}
                </div>

                {/* Create new */}
                <div className="border-t border-border-primary px-4 py-2 shrink-0">
                    {creating ? (
                        <div className="flex items-center gap-2">
                            <input
                                type="text"
                                value={newName}
                                onInput={(e) => setNewName((e.target as HTMLInputElement).value)}
                                onKeyDown={(e) => {
                                    if (e.key === 'Enter') handleCreate();
                                    if (e.key === 'Escape') setCreating(false);
                                }}
                                placeholder="New playlist name..."
                                className="flex-1 bg-bg-primary border border-border-primary text-fg-primary font-mono text-sm px-2 py-1 outline-none focus:border-accent-primary"
                                disabled={loading}
                                // biome-ignore lint: autofocus intentional
                                autoFocus
                            />
                            <button
                                type="button"
                                className="px-3 py-1 bg-accent-primary text-fg-primary font-mono text-sm border-none cursor-pointer"
                                onClick={handleCreate}
                                disabled={loading}
                            >
                                Create
                            </button>
                        </div>
                    ) : (
                        <button
                            type="button"
                            className="flex items-center gap-2 w-full px-0 py-1 bg-transparent border-none text-fg-secondary text-sm cursor-pointer transition-colors duration-150 hover:text-accent-primary"
                            onClick={() => setCreating(true)}
                        >
                            <Plus size={16} />
                            Create New Playlist
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
}
