import { useStore } from '@nanostores/preact';
import { ChevronLeft, ChevronRight, Plus } from 'lucide-preact';
import { useEffect, useMemo } from 'preact/hooks';
import { LibraryList } from '../components/LibraryList';
import { SwipeableTrackItem } from '../components/SwipeableTrackItem';
import { useAddToQueue } from '../hooks/useAddToQueue';
import { libraryRevision } from '../services/library-cache';
import type { LibraryRef } from '../services/mopidy';
import * as mopidy from '../services/mopidy';
import {
    currentUri,
    libraryError,
    libraryItems,
    libraryLoading,
    libraryPath,
    loadLibrary,
    navigateBack,
    navigateTo,
    navigateToIndex,
    saveLibraryScroll,
} from '../stores/library';
import { queue } from '../stores/queue';
import { toastError } from '../stores/toast';
import { getLibraryIcon } from '../utils/icons';

export function LibraryView() {
    const items = useStore(libraryItems);
    const path = useStore(libraryPath);
    const loading = useStore(libraryLoading);
    const error = useStore(libraryError);
    const uri = useStore(currentUri);
    const revision = useStore(libraryRevision);
    const queueTracks = useStore(queue);
    const { addToQueue, addNext } = useAddToQueue();

    const queuedUris = useMemo(() => {
        return new Set(queueTracks.map((t) => t.track.uri));
    }, [queueTracks]);

    // biome-ignore lint/correctness/useExhaustiveDependencies: server revisions explicitly invalidate the loaded folder
    useEffect(() => {
        void loadLibrary();
    }, [revision]);

    const handleItemClick = async (item: LibraryRef) => {
        if (loading) return;
        if (item.type !== 'track') {
            navigateTo(item.uri, item.name);
        }
    };

    const handleAddToQueue = async (item: LibraryRef, e: Event) => {
        e.stopPropagation();
        try {
            if (item.type === 'track') {
                await addToQueue(item.uri);
            } else {
                const tracksMap = await mopidy.lookup([item.uri]);
                const tracks = tracksMap.get(item.uri) || [];
                if (tracks.length > 0) {
                    await addToQueue(tracks.map((t) => t.uri));
                }
            }
        } catch (err) {
            console.error('Failed to add to queue:', err);
            toastError(`Could not add ${item.name} to queue`);
        }
    };

    const handleAddNext = async (item: LibraryRef, e: Event) => {
        e.stopPropagation();
        try {
            if (item.type === 'track') {
                await addNext(item.uri);
            } else {
                const tracksMap = await mopidy.lookup([item.uri]);
                const tracks = tracksMap.get(item.uri) || [];
                if (tracks.length > 0) {
                    await addNext(tracks.map((t) => t.uri));
                }
            }
        } catch (err) {
            console.error('Failed to add next:', err);
            toastError(`Could not add ${item.name} to queue`);
        }
    };

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Breadcrumb navigation */}
            <div className="flex items-center gap-1 px-2 py-1 border-b border-border-primary shrink-0 overflow-x-auto bg-bg-secondary">
                {path.length > 1 && (
                    <button
                        type="button"
                        className="flex items-center justify-center w-8 h-8 bg-transparent border-none text-fg-secondary cursor-pointer shrink-0 transition-colors duration-150 hover:text-accent-primary disabled:opacity-50 disabled:cursor-not-allowed"
                        onClick={navigateBack}
                        aria-label="Go back"
                    >
                        <ChevronLeft size={20} />
                    </button>
                )}
                <div className="flex items-center gap-0 overflow-x-auto">
                    {path.map((crumb, index) => (
                        <button
                            type="button"
                            key={crumb.uri ?? ''}
                            className={`flex items-center gap-0.5 px-1.5 py-1 bg-transparent border-none text-sm whitespace-nowrap cursor-pointer transition-colors duration-150 hover:text-accent-primary disabled:cursor-default ${index === path.length - 1 ? 'text-accent-primary' : 'text-fg-secondary'}`}
                            onClick={() => navigateToIndex(index)}
                            disabled={index === path.length - 1}
                        >
                            {crumb.name}
                            {index < path.length - 1 && (
                                <ChevronRight size={12} className="text-fg-tertiary shrink-0" />
                            )}
                        </button>
                    ))}
                </div>
            </div>

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
                        className="px-4 py-2 bg-bg-tertiary border-4 border-border-primary text-fg-primary font-mono text-sm cursor-pointer transition-all duration-150 uppercase hover:text-accent-primary hover:border-accent-primary"
                        onClick={() => loadLibrary()}
                    >
                        Retry
                    </button>
                </div>
            ) : items.length === 0 ? (
                <div className="flex flex-col items-center justify-center min-h-[50vh] gap-2 text-fg-secondary text-center px-8">
                    <p>No items found</p>
                </div>
            ) : (
                <LibraryList
                    key={uri ?? ''}
                    items={items}
                    initialScroll={path.at(-1)?.scrollTop ?? 0}
                    onScroll={saveLibraryScroll}
                    renderItem={(item) =>
                        item.type === 'track' ? (
                            <SwipeableTrackItem
                                key={item.uri}
                                track={{ name: item.name, duration: undefined, artists: undefined }}
                                trackUri={item.uri}
                                isQueued={queuedUris.has(item.uri)}
                                disabled={loading}
                                onAdd={() => handleAddToQueue(item, new Event('click'))}
                                onAddNext={() => handleAddNext(item, new Event('click'))}
                                icon={getLibraryIcon(item.type)}
                                showDuration={false}
                                customMeta="track"
                                leftLabel="+ Add Next"
                                rightLabel="+ Add"
                            />
                        ) : (
                            // biome-ignore lint/a11y/useSemanticElements: the row contains its own <button>s, so it cannot be a <button>
                            <div
                                key={item.uri}
                                role="button"
                                tabIndex={0}
                                className={`flex items-center gap-2 px-4 py-2 bg-bg-primary border-b-2 border-border-secondary min-h-12 w-full cursor-pointer transition-all duration-150 active:bg-bg-tertiary active:translate-y-px ${loading ? 'cursor-not-allowed opacity-60' : ''}`}
                                onClick={() => handleItemClick(item)}
                                onKeyDown={(e) => {
                                    if (e.key === 'Enter' || e.key === ' ') {
                                        e.preventDefault();
                                        (e.currentTarget as HTMLElement).click();
                                    }
                                }}
                            >
                                <div
                                    className={`flex items-center justify-center w-6 h-6 shrink-0 ${item.type === 'directory' || item.type === 'artist' ? 'text-fg-secondary' : item.type === 'album' ? 'text-accent-secondary' : item.type === 'playlist' ? 'text-accent-dim' : 'text-fg-tertiary'}`}
                                >
                                    {getLibraryIcon(item.type)}
                                </div>
                                <div className="flex-1 min-w-0 flex flex-col gap-0.5">
                                    <div className="text-fg-primary truncate">{item.name}</div>
                                    <div className="text-sm text-fg-tertiary capitalize">
                                        {item.type}
                                    </div>
                                </div>
                                {item.type !== 'directory' && (
                                    <button
                                        type="button"
                                        className="btn-icon"
                                        onClick={(e) => handleAddToQueue(item, e)}
                                        aria-label={`Add ${item.name} to queue`}
                                        title="Add to queue"
                                    >
                                        <Plus size={18} />
                                    </button>
                                )}
                                <ChevronRight size={18} className="text-fg-tertiary shrink-0" />
                            </div>
                        )
                    }
                />
            )}
        </div>
    );
}
