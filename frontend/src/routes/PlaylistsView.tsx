import { useStore } from '@nanostores/preact';
import { ListMusic, Plus, Trash2 } from 'lucide-preact';
import { useEffect, useState } from 'preact/hooks';
import { useLocation } from 'wouter';
import { confirm } from '../components/ConfirmModal';
import * as playlistService from '../services/playlists';
import {
    playlists,
    playlistsError,
    playlistsLoading,
    setPlaylists,
    setPlaylistsError,
    setPlaylistsLoading,
} from '../stores/playlists';
import { toastError } from '../stores/toast';

export function PlaylistsView() {
    const items = useStore(playlists);
    const loading = useStore(playlistsLoading);
    const error = useStore(playlistsError);
    const [, setLocation] = useLocation();
    const [creating, setCreating] = useState(false);
    const [newName, setNewName] = useState('');

    const loadPlaylists = async () => {
        setPlaylistsLoading(true);
        setPlaylistsError(null);
        try {
            const data = await playlistService.listPlaylists();
            setPlaylists(data);
        } catch (err) {
            setPlaylistsError(err instanceof Error ? err.message : 'Failed to load playlists');
        } finally {
            setPlaylistsLoading(false);
        }
    };

    useEffect(() => {
        loadPlaylists();
    }, []);

    const handleCreate = async () => {
        const name = newName.trim();
        if (!name) return;
        try {
            await playlistService.createPlaylist(name);
            setNewName('');
            setCreating(false);
            await loadPlaylists();
        } catch (err) {
            console.error('Failed to create playlist:', err);
            toastError('Could not create playlist');
        }
    };

    const handleDelete = (id: number, name: string, e: Event) => {
        e.stopPropagation();
        confirm({
            title: 'Delete Playlist',
            message: `Delete "${name}" and all its tracks?`,
            confirmLabel: 'Delete',
            destructive: true,
            onConfirm: async () => {
                try {
                    await playlistService.deletePlaylist(id);
                    await loadPlaylists();
                } catch (err) {
                    console.error('Failed to delete playlist:', err);
                    toastError('Could not delete playlist');
                }
            },
        });
    };

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Header bar */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-border-primary shrink-0 bg-bg-secondary">
                <span className="text-fg-secondary text-sm">Playlists</span>
                <button
                    type="button"
                    className="flex items-center gap-1 px-2 py-1 bg-transparent border border-border-primary text-fg-secondary text-sm font-mono cursor-pointer transition-all duration-150 hover:text-accent-primary hover:border-accent-primary"
                    onClick={() => setCreating(!creating)}
                >
                    <Plus size={14} />
                    New
                </button>
            </div>

            {/* Inline create form */}
            {creating && (
                <div className="flex items-center gap-2 px-4 py-2 border-b border-border-primary bg-bg-tertiary">
                    <input
                        type="text"
                        value={newName}
                        onInput={(e) => setNewName((e.target as HTMLInputElement).value)}
                        onKeyDown={(e) => {
                            if (e.key === 'Enter') handleCreate();
                            if (e.key === 'Escape') setCreating(false);
                        }}
                        placeholder="Playlist name..."
                        className="flex-1 bg-bg-primary border border-border-primary text-fg-primary font-mono text-sm px-2 py-1 outline-none focus:border-accent-primary"
                        // biome-ignore lint: autofocus is intentional for inline create form
                        autoFocus
                    />
                    <button
                        type="button"
                        className="px-3 py-1 bg-accent-primary text-fg-primary font-mono text-sm border-none cursor-pointer"
                        onClick={handleCreate}
                    >
                        Create
                    </button>
                </div>
            )}

            {/* Content */}
            {loading && items.length === 0 ? (
                <div className="flex items-center justify-center min-h-[50vh] text-fg-secondary">
                    Loading...
                </div>
            ) : error ? (
                <div className="flex flex-col items-center justify-center min-h-[50vh] gap-4 text-error text-center px-8">
                    <p>{error}</p>
                    <button
                        type="button"
                        className="px-4 py-2 bg-bg-tertiary border border-border-primary text-fg-primary font-mono text-sm cursor-pointer transition-all duration-150 hover:text-accent-primary hover:border-accent-primary"
                        onClick={loadPlaylists}
                    >
                        Retry
                    </button>
                </div>
            ) : items.length === 0 ? (
                <div className="flex flex-col items-center justify-center min-h-[50vh] gap-2 text-fg-secondary text-center px-8">
                    <ListMusic size={32} className="text-fg-tertiary" />
                    <p>No playlists yet</p>
                    <p className="text-sm text-fg-tertiary">Tap "New" to create one</p>
                </div>
            ) : (
                <div className="flex-1 overflow-y-auto overscroll-y-contain pb-2">
                    {items.map((playlist) => (
                        // biome-ignore lint/a11y/useSemanticElements: the row contains its own <button>s, so it cannot be a <button>
                        <div
                            key={playlist.id}
                            role="button"
                            tabIndex={0}
                            className="flex items-center gap-3 px-4 py-3 bg-bg-primary border-b-2 border-border-secondary cursor-pointer transition-all duration-150 active:bg-bg-tertiary"
                            onClick={() => setLocation(`/playlists/${playlist.id}`)}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter' || e.key === ' ') {
                                    e.preventDefault();
                                    (e.currentTarget as HTMLElement).click();
                                }
                            }}
                        >
                            <ListMusic size={20} className="text-fg-tertiary shrink-0" />
                            <div className="flex-1 min-w-0">
                                <div className="text-fg-primary truncate">{playlist.name}</div>
                                {playlist.description && (
                                    <div className="text-sm text-fg-tertiary truncate">
                                        {playlist.description}
                                    </div>
                                )}
                            </div>
                            <button
                                type="button"
                                className="flex items-center justify-center w-8 h-8 bg-transparent border border-border-primary text-fg-tertiary cursor-pointer shrink-0 transition-all duration-150 hover:text-error hover:border-error"
                                onClick={(e) => handleDelete(playlist.id, playlist.name, e)}
                                aria-label={`Delete ${playlist.name}`}
                            >
                                <Trash2 size={14} />
                            </button>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}
