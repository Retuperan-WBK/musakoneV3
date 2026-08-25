import { useStore } from '@nanostores/preact';
import { ChevronRight, Disc, Music, Plus, Search, User, X } from 'lucide-preact';
import { useCallback, useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { useLocation } from 'wouter';
import { SwipeableTrackItem } from '../components/SwipeableTrackItem';
import { useAddToQueue } from '../hooks/useAddToQueue';
import * as mopidy from '../services/mopidy';
import { connectionStatus } from '../stores/connection';
import { navigateTo, resetLibrary } from '../stores/library';
import { queue } from '../stores/queue';
import {
    clearSearch,
    searchAlbums,
    searchArtists,
    searchError,
    searchLoading,
    searchQuery,
    searchTab,
    searchTracks,
    setSearchError,
    setSearchLoading,
    setSearchQuery,
    setSearchResults,
    setSearchTab,
} from '../stores/search';
import { toastError } from '../stores/toast';
import type { Album, Artist, Track } from '../types';

export function SearchView() {
    const query = useStore(searchQuery);
    const tracks = useStore(searchTracks);
    const artists = useStore(searchArtists);
    const albums = useStore(searchAlbums);
    const loading = useStore(searchLoading);
    const error = useStore(searchError);
    const tab = useStore(searchTab);
    const queueTracks = useStore(queue);
    const connStatus = useStore(connectionStatus);
    const [inputValue, setInputValue] = useState(query);
    const [, setLocation] = useLocation();
    const hasInitialized = useRef(false);
    const pendingSearch = useRef<string | null>(null);
    const searchSeq = useRef(0);
    const { addToQueue, addNext } = useAddToQueue();

    const updateUrl = useCallback((searchQuery: string, currentTab: string) => {
        const params = new URLSearchParams();
        if (searchQuery) {
            params.set('q', searchQuery);
            params.set('tab', currentTab);
        }
        const search = params.toString();
        const newUrl = search ? `/search?${search}` : '/search';
        window.history.replaceState(null, '', newUrl);
    }, []);

    const queuedUris = useMemo(() => {
        return new Set(queueTracks.map((t) => t.track.uri));
    }, [queueTracks]);

    const handleSearch = useCallback(
        async (searchValue: string) => {
            const trimmed = searchValue.trim();
            if (!trimmed) {
                clearSearch();
                updateUrl('', tab);
                return;
            }

            setSearchQuery(trimmed);
            setSearchLoading(true);
            setSearchError(null);
            updateUrl(trimmed, tab);

            // Only the most recent search may update the results
            const seq = ++searchSeq.current;
            try {
                const results = await mopidy.search(trimmed);
                if (seq !== searchSeq.current) return;
                setSearchResults(results.tracks, results.artists, results.albums);
            } catch (err) {
                if (seq !== searchSeq.current) return;
                console.error('Search failed:', err);
                setSearchError(err instanceof Error ? err.message : 'Search failed');
            } finally {
                if (seq === searchSeq.current) setSearchLoading(false);
            }
        },
        [tab, updateUrl]
    );

    useEffect(() => {
        if (hasInitialized.current) return;
        hasInitialized.current = true;

        const params = new URLSearchParams(window.location.search);
        const urlQuery = params.get('q');
        const urlTab = params.get('tab') as 'tracks' | 'artists' | 'albums' | null;

        if (urlTab && ['tracks', 'artists', 'albums'].includes(urlTab)) {
            setSearchTab(urlTab);
        }

        if (urlQuery) {
            setInputValue(urlQuery);
            pendingSearch.current = urlQuery;
        }
    }, []);

    useEffect(() => {
        if (connStatus === 'connected' && pendingSearch.current) {
            const queryToSearch = pendingSearch.current;
            pendingSearch.current = null;
            handleSearch(queryToSearch);
        }
    }, [connStatus, handleSearch]);

    const handleSubmit = (e: Event) => {
        e.preventDefault();
        handleSearch(inputValue);
    };

    const handleClear = () => {
        setInputValue('');
        clearSearch();
        updateUrl('', tab);
    };

    const handleAddTrack = async (track: Track) => {
        try {
            await addToQueue(track.uri);
        } catch (err) {
            console.error('Failed to add track:', err);
            toastError(`Could not add ${track.name} to queue`);
        }
    };

    const handleAddTrackNext = async (track: Track) => {
        try {
            await addNext(track.uri);
        } catch (err) {
            console.error('Failed to add track next:', err);
            toastError(`Could not add ${track.name} to queue`);
        }
    };

    const handleAddArtist = async (artist: Artist) => {
        try {
            const tracksMap = await mopidy.lookup([artist.uri]);
            const artistTracks = tracksMap.get(artist.uri) || [];
            if (artistTracks.length > 0) {
                await addToQueue(artistTracks.map((t) => t.uri));
            }
        } catch (err) {
            console.error('Failed to add artist tracks:', err);
            toastError(`Could not add ${artist.name} to queue`);
        }
    };

    const handleAddAlbum = async (album: Album) => {
        try {
            const tracksMap = await mopidy.lookup([album.uri]);
            const albumTracks = tracksMap.get(album.uri) || [];
            if (albumTracks.length > 0) {
                await addToQueue(albumTracks.map((t) => t.uri));
            }
        } catch (err) {
            console.error('Failed to add album tracks:', err);
            toastError(`Could not add ${album.name} to queue`);
        }
    };

    const handleArtistClick = (artist: Artist) => {
        resetLibrary();
        navigateTo(artist.uri, artist.name);
        setLocation('/library');
    };

    const handleAlbumClick = (album: Album) => {
        resetLibrary();
        navigateTo(album.uri, album.name);
        setLocation('/library');
    };

    const hasResults = tracks.length > 0 || artists.length > 0 || albums.length > 0;

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Search input */}
            <form
                className="flex gap-0 border-b border-border-primary shrink-0"
                onSubmit={handleSubmit}
                autoComplete="off"
            >
                <div className="flex-1 flex items-center bg-bg-secondary border border-border-primary focus-within:border-accent-primary transition-colors duration-150">
                    <Search size={18} className="text-fg-tertiary shrink-0" />
                    <input
                        type="search"
                        className="flex-1 min-h-12 pl-2 bg-transparent border-none text-fg-primary font-mono text-base outline-none placeholder:text-fg-tertiary"
                        placeholder="Search music..."
                        value={inputValue}
                        onInput={(e) => setInputValue((e.target as HTMLInputElement).value)}
                        autoComplete="off"
                        autoCorrect="off"
                        autoCapitalize="off"
                        enterKeyHint="search"
                        data-form-type="other"
                    />
                    {inputValue && (
                        <button
                            type="button"
                            className="flex items-center justify-center w-8 h-8 bg-transparent border-none text-fg-tertiary cursor-pointer transition-colors duration-150 hover:text-fg-primary"
                            onClick={handleClear}
                            aria-label="Clear search"
                        >
                            <X size={18} />
                        </button>
                    )}
                </div>
                <button
                    type="submit"
                    className="min-w-[130px] min-h-12 px-6 bg-accent-primary border-none text-bg-primary font-mono text-sm font-medium cursor-pointer transition-all duration-150 whitespace-nowrap hover:brightness-110 active:brightness-90 disabled:opacity-70 disabled:cursor-not-allowed"
                    disabled={loading}
                >
                    {loading ? 'Searching...' : 'Search'}
                </button>
            </form>

            {/* Tabs */}
            {hasResults && (
                <div className="flex border-b border-border-primary overflow-x-auto shrink-0 scrollbar-none">
                    <button
                        type="button"
                        className={`flex items-center gap-1 shrink-0 min-h-12 px-6 bg-transparent border-none border-b-2 border-b-transparent text-fg-secondary font-mono text-sm cursor-pointer whitespace-nowrap transition-all duration-150 hover:text-fg-primary hover:bg-bg-secondary ${tab === 'tracks' ? 'text-accent-primary border-b-accent-primary' : ''}`}
                        onClick={() => {
                            setSearchTab('tracks');
                            updateUrl(query, 'tracks');
                        }}
                    >
                        <Music size={16} />
                        Tracks ({tracks.length})
                    </button>
                    <button
                        type="button"
                        className={`flex items-center gap-1 shrink-0 min-h-12 px-6 bg-transparent border-none border-b-2 border-b-transparent text-fg-secondary font-mono text-sm cursor-pointer whitespace-nowrap transition-all duration-150 hover:text-fg-primary hover:bg-bg-secondary ${tab === 'artists' ? 'text-accent-primary border-b-accent-primary' : ''}`}
                        onClick={() => {
                            setSearchTab('artists');
                            updateUrl(query, 'artists');
                        }}
                    >
                        <User size={16} />
                        Artists ({artists.length})
                    </button>
                    <button
                        type="button"
                        className={`flex items-center gap-1 shrink-0 min-h-12 px-6 bg-transparent border-none border-b-2 border-b-transparent text-fg-secondary font-mono text-sm cursor-pointer whitespace-nowrap transition-all duration-150 hover:text-fg-primary hover:bg-bg-secondary ${tab === 'albums' ? 'text-accent-primary border-b-accent-primary' : ''}`}
                        onClick={() => {
                            setSearchTab('albums');
                            updateUrl(query, 'albums');
                        }}
                    >
                        <Disc size={16} />
                        Albums ({albums.length})
                    </button>
                </div>
            )}

            {/* Results */}
            {loading ? (
                <div className="flex items-center justify-center min-h-[50vh] text-fg-secondary">
                    Searching...
                </div>
            ) : error ? (
                <div className="flex flex-col items-center justify-center min-h-[50vh] gap-4 text-error text-center px-8">
                    <p>{error}</p>
                    <button
                        type="button"
                        className="px-4 py-2 bg-bg-secondary border border-border-primary text-fg-secondary font-mono text-sm cursor-pointer transition-all duration-150 hover:text-accent-primary hover:border-accent-primary"
                        onClick={() => handleSearch(query)}
                    >
                        Retry
                    </button>
                </div>
            ) : !query ? (
                <div className="flex flex-col items-center justify-center min-h-[50vh] gap-4 text-fg-secondary text-center px-8">
                    <Search size={48} className="text-fg-tertiary opacity-50" />
                    <p>Search for tracks, artists, or albums</p>
                </div>
            ) : !hasResults ? (
                <div className="flex flex-col items-center justify-center min-h-[50vh] gap-2 text-fg-secondary text-center px-8">
                    <p>No results found for "{query}"</p>
                </div>
            ) : (
                <div className="flex-1 overflow-y-auto overscroll-y-contain pb-2">
                    {tab === 'tracks' && (
                        <div className="flex flex-col">
                            {tracks.map((track) => (
                                <SwipeableTrackItem
                                    key={track.uri}
                                    track={track}
                                    trackUri={track.uri}
                                    isQueued={queuedUris.has(track.uri)}
                                    onAdd={() => handleAddTrack(track)}
                                    onAddNext={() => handleAddTrackNext(track)}
                                    showAlbum={true}
                                    leftLabel="+ Queue Next"
                                    rightLabel="+ Queue"
                                />
                            ))}
                        </div>
                    )}
                    {tab === 'artists' && (
                        <div className="flex flex-col">
                            {artists.map((artist) => (
                                // biome-ignore lint/a11y/useSemanticElements: the row contains its own <button>s, so it cannot be a <button>
                                <div
                                    key={artist.uri}
                                    role="button"
                                    tabIndex={0}
                                    className="flex items-center gap-2 px-2 py-1 bg-bg-primary min-h-11 transition-transform duration-100 cursor-pointer active:bg-bg-secondary"
                                    onClick={() => handleArtistClick(artist)}
                                    onKeyDown={(e) => {
                                        if (e.key === 'Enter' || e.key === ' ') {
                                            e.preventDefault();
                                            (e.currentTarget as HTMLElement).click();
                                        }
                                    }}
                                >
                                    <div className="flex items-center justify-center w-6 h-6 text-fg-secondary shrink-0">
                                        <User size={20} />
                                    </div>
                                    <div className="flex-1 min-w-0 flex flex-col gap-0.5">
                                        <div className="text-fg-primary truncate">
                                            {artist.name}
                                        </div>
                                        <div className="text-sm text-fg-secondary truncate">
                                            Artist
                                        </div>
                                    </div>
                                    <button
                                        type="button"
                                        className="btn-icon"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            handleAddArtist(artist);
                                        }}
                                        aria-label={`Add all ${artist.name} tracks to queue`}
                                    >
                                        <Plus size={18} />
                                    </button>
                                    <ChevronRight size={18} className="text-fg-tertiary shrink-0" />
                                </div>
                            ))}
                        </div>
                    )}
                    {tab === 'albums' && (
                        <div className="flex flex-col">
                            {albums.map((album) => (
                                // biome-ignore lint/a11y/useSemanticElements: the row contains its own <button>s, so it cannot be a <button>
                                <div
                                    key={album.uri}
                                    role="button"
                                    tabIndex={0}
                                    className="flex items-center gap-2 px-2 py-1 bg-bg-primary min-h-11 transition-transform duration-100 cursor-pointer active:bg-bg-secondary"
                                    onClick={() => handleAlbumClick(album)}
                                    onKeyDown={(e) => {
                                        if (e.key === 'Enter' || e.key === ' ') {
                                            e.preventDefault();
                                            (e.currentTarget as HTMLElement).click();
                                        }
                                    }}
                                >
                                    <div className="flex items-center justify-center w-6 h-6 text-accent-secondary shrink-0">
                                        <Disc size={20} />
                                    </div>
                                    <div className="flex-1 min-w-0 flex flex-col gap-0.5">
                                        <div className="text-fg-primary truncate">{album.name}</div>
                                        <div className="text-sm text-fg-secondary truncate">
                                            {album.artists?.map((a) => a.name).join(', ') ||
                                                'Unknown Artist'}
                                        </div>
                                    </div>
                                    <button
                                        type="button"
                                        className="btn-icon"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            handleAddAlbum(album);
                                        }}
                                        aria-label={`Add ${album.name} to queue`}
                                    >
                                        <Plus size={18} />
                                    </button>
                                    <ChevronRight size={18} className="text-fg-tertiary shrink-0" />
                                </div>
                            ))}
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}
