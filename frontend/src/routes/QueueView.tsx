import { useStore } from '@nanostores/preact';
import { GripVertical, MoreVertical, Play, RefreshCw, Shuffle, Trash2 } from 'lucide-preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import { confirm } from '../components/ConfirmModal';
import { TrackItem } from '../components/TrackItem';
import * as mopidy from '../services/mopidy';
import { currentTrack } from '../stores/player';
import { queue, scrollToCurrentTrack, setQueue } from '../stores/queue';
import { toastError } from '../stores/toast';

/** Distance from the list edge (px) inside which dragging auto-scrolls the list */
const AUTOSCROLL_ZONE = 64;
/** Auto-scroll speed at the very edge, in px per animation frame */
const AUTOSCROLL_MAX_SPEED = 14;
/** Vertical offset so the drag ghost sits under the finger rather than below it */
const GHOST_OFFSET = 24;
/** Two taps on the same row within this window count as a double-tap (native dblclick is unreliable on touch) */
const DOUBLE_TAP_MS = 350;
/** Row actions and the queue menu close by themselves after this much inactivity */
const AUTO_CLOSE_MS = 5000;

interface DragSession {
    pointerId: number;
    /** Index of the row being dragged */
    from: number;
    /** Insertion index 0..length: drop *before* item `to` (`length` = end of list) */
    to: number;
    lastY: number;
    /** Height of the dragged row, used to slide the other rows out of the way */
    rowHeight: number;
    /** Height of the list's bottom padding, i.e. the area hidden behind the mini player */
    bottomCover: number;
    raf: number | null;
}

function formatTotalDuration(ms: number): string {
    const minutes = Math.round(ms / 60000);
    if (minutes < 60) return `${minutes} min`;
    return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

/** Index a track dragged from `from` and dropped at insertion point `to` ends up at */
function finalIndex(from: number, to: number): number {
    return to > from ? to - 1 : to;
}

export function QueueView() {
    const queueTracks = useStore(queue);
    const current = useStore(currentTrack);
    const scrollTrigger = useStore(scrollToCurrentTrack);
    const [loading, setLoading] = useState(false);
    const [currentTlid, setCurrentTlid] = useState<number | null>(null);
    const [selectedTlid, setSelectedTlid] = useState<number | null>(null);
    const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
    const [dropIndex, setDropIndex] = useState<number | null>(null);
    const [dragRowHeight, setDragRowHeight] = useState(0);
    const [menuOpen, setMenuOpen] = useState(false);
    const listRef = useRef<HTMLDivElement>(null);
    const currentTrackRef = useRef<HTMLDivElement>(null);
    const ghostRef = useRef<HTMLDivElement>(null);
    const menuRef = useRef<HTMLDivElement>(null);
    const dragRef = useRef<DragSession | null>(null);
    const didInitialScroll = useRef(false);
    const lastTap = useRef<{ tlid: number; time: number } | null>(null);

    const loadQueue = async () => {
        setLoading(true);
        try {
            const [tracks, tlid] = await Promise.all([
                mopidy.getTracklist(),
                mopidy.getCurrentTlid(),
            ]);
            setQueue(tracks);
            setCurrentTlid(tlid);
        } catch (err) {
            console.error('Failed to load queue:', err);
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        loadQueue();
    }, []);

    useEffect(() => {
        let cancelled = false;
        if (current) {
            mopidy
                .getCurrentTlid()
                .then((tlid) => {
                    if (!cancelled) setCurrentTlid(tlid);
                })
                .catch((err) => console.error('Failed to get current tlid:', err));
        }
        return () => {
            cancelled = true;
        };
    }, [current]);

    /**
     * Scroll the queue list so that `row` is vertically centred in its visible area.
     * Scrolls only the list container. (`scrollIntoView` also scrolls every scrollable
     * ancestor – including the overflow:hidden <body> – which shifted the whole app up.)
     */
    const scrollListToRow = (row: HTMLElement, behavior: ScrollBehavior) => {
        const list = listRef.current;
        if (!list) return;
        const bottomCover = Number.parseFloat(getComputedStyle(list).paddingBottom) || 0;
        const visibleHeight = list.clientHeight - bottomCover;
        const target = row.offsetTop - visibleHeight / 2 + row.offsetHeight / 2;
        list.scrollTo({ top: Math.max(0, target), behavior });
    };

    // Explicit "jump to current" from the mini player
    useEffect(() => {
        if (scrollTrigger > 0 && currentTrackRef.current) {
            scrollListToRow(currentTrackRef.current, 'smooth');
        }
    }, [scrollTrigger]);

    // On first render with a known current track, start the list at that track
    useEffect(() => {
        if (!didInitialScroll.current && currentTlid !== null && currentTrackRef.current) {
            didInitialScroll.current = true;
            scrollListToRow(currentTrackRef.current, 'auto');
        }
    }, [currentTlid, queueTracks.length]);

    // Row action overlay: close on any tap outside the list rows, or by itself after a while
    useEffect(() => {
        if (selectedTlid === null) return;

        const handleClickOutside = (e: MouseEvent) => {
            if ((e.target as Element | null)?.closest('[data-queue-row]')) return;
            setSelectedTlid(null);
        };
        const listen = setTimeout(() => {
            document.addEventListener('click', handleClickOutside);
        }, 0);
        const autoClose = setTimeout(() => setSelectedTlid(null), AUTO_CLOSE_MS);
        return () => {
            clearTimeout(listen);
            clearTimeout(autoClose);
            document.removeEventListener('click', handleClickOutside);
        };
    }, [selectedTlid]);

    // Queue actions menu: close on outside tap, or by itself after a while
    useEffect(() => {
        if (!menuOpen) return;

        const handleClickOutside = (e: MouseEvent) => {
            if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
                setMenuOpen(false);
            }
        };
        document.addEventListener('click', handleClickOutside);
        const autoClose = setTimeout(() => setMenuOpen(false), AUTO_CLOSE_MS);
        return () => {
            clearTimeout(autoClose);
            document.removeEventListener('click', handleClickOutside);
        };
    }, [menuOpen]);

    // Position the ghost as soon as it is mounted, and stop auto-scroll on unmount
    useEffect(() => {
        if (draggedIndex !== null && dragRef.current) {
            positionGhost(dragRef.current.lastY);
        }
    }, [draggedIndex]);

    useEffect(() => {
        return () => {
            if (dragRef.current?.raf) cancelAnimationFrame(dragRef.current.raf);
        };
    }, []);

    const handlePlayTrack = async (tlid: number) => {
        try {
            await mopidy.play(tlid);
            setCurrentTlid(tlid);
        } catch (err) {
            console.error('Failed to play track:', err);
            toastError('Could not start playback');
        }
    };

    const handleRemoveTrack = async (tlid: number) => {
        try {
            await mopidy.removeFromTracklist([tlid]);
            setQueue(queue.get().filter((t) => t.tlid !== tlid));
            setSelectedTlid((selected) => (selected === tlid ? null : selected));
        } catch (err) {
            console.error('Failed to remove track:', err);
            toastError('Could not remove track');
        }
    };

    /**
     * Row actions open only on a double-tap – a single accidental tap must never do anything.
     * Detected manually: browsers don't dispatch `dblclick` consistently for touch input.
     */
    const handleTrackTap = (tlid: number) => {
        if (dragRef.current) return;
        const now = Date.now();
        const previous = lastTap.current;
        lastTap.current = { tlid, time: now };
        if (previous && previous.tlid === tlid && now - previous.time < DOUBLE_TAP_MS) {
            lastTap.current = null;
            setSelectedTlid((selected) => (selected === tlid ? null : tlid));
        }
    };

    const handleClearQueue = () => {
        setMenuOpen(false);
        confirm({
            title: 'Clear Queue',
            message: `Remove all ${queueTracks.length} tracks from the queue?`,
            confirmLabel: 'Clear',
            destructive: true,
            onConfirm: async () => {
                try {
                    await mopidy.clearTracklist();
                    await loadQueue();
                } catch (err) {
                    console.error('Failed to clear queue:', err);
                    toastError('Could not clear the queue');
                }
            },
        });
    };

    const handleShuffle = () => {
        setMenuOpen(false);
        confirm({
            title: 'Shuffle Queue',
            message: 'Randomize the order of all tracks in the queue?',
            confirmLabel: 'Shuffle',
            onConfirm: async () => {
                try {
                    await mopidy.shuffleTracklist();
                    await loadQueue();
                } catch (err) {
                    console.error('Failed to shuffle queue:', err);
                    toastError('Could not shuffle the queue');
                }
            },
        });
    };

    /** Move the track at `from` so that it ends up at index `toIndex`; optimistic, reverts on failure */
    const moveTrack = async (from: number, toIndex: number) => {
        const tracks = [...queue.get()];
        if (
            from === toIndex ||
            from < 0 ||
            from >= tracks.length ||
            toIndex < 0 ||
            toIndex >= tracks.length
        )
            return;
        const [moved] = tracks.splice(from, 1);
        if (!moved) return;
        tracks.splice(toIndex, 0, moved);
        setQueue(tracks);

        try {
            // core.tracklist.move(start, end, to_position): moves tracks[start:end] so the
            // first of them ends up at to_position in the list *after* removal.
            await mopidy.moveTrack(from, from + 1, toIndex);
        } catch (err) {
            console.error('Failed to move track:', err);
            toastError('Could not reorder the queue');
            await loadQueue();
        }
    };

    // ---- Drag and drop (pointer events: works with touch, mouse and pen) ----

    const positionGhost = (y: number) => {
        if (ghostRef.current) {
            ghostRef.current.style.transform = `translateY(${Math.round(y - GHOST_OFFSET)}px)`;
        }
    };

    /**
     * Insertion index for pointer position `y`: before the first row whose midpoint is below it.
     * Uses layout positions (offsetTop), not bounding rects, so the slide animation applied to
     * rows while dragging cannot feed back into the calculation.
     */
    const computeDropIndex = (y: number): number => {
        const list = listRef.current;
        if (!list) return 0;
        const listY = y - list.getBoundingClientRect().top + list.scrollTop;
        const rows = list.querySelectorAll<HTMLElement>('[data-queue-row]');
        for (let i = 0; i < rows.length; i++) {
            const row = rows[i]!;
            if (listY < row.offsetTop + row.offsetHeight / 2) return i;
        }
        return rows.length;
    };

    const updateDropIndex = (y: number) => {
        const session = dragRef.current;
        if (!session) return;
        const to = computeDropIndex(y);
        if (to !== session.to) {
            session.to = to;
            setDropIndex(to);
        }
    };

    const autoScrollStep = () => {
        const session = dragRef.current;
        const list = listRef.current;
        if (!session || !list) return;

        const rect = list.getBoundingClientRect();
        const top = rect.top;
        const bottom = rect.bottom - session.bottomCover;
        let speed = 0;
        if (session.lastY < top + AUTOSCROLL_ZONE) {
            speed =
                -AUTOSCROLL_MAX_SPEED *
                Math.min(1, (top + AUTOSCROLL_ZONE - session.lastY) / AUTOSCROLL_ZONE);
        } else if (session.lastY > bottom - AUTOSCROLL_ZONE) {
            speed =
                AUTOSCROLL_MAX_SPEED *
                Math.min(1, (session.lastY - (bottom - AUTOSCROLL_ZONE)) / AUTOSCROLL_ZONE);
        }

        if (speed !== 0) {
            const before = list.scrollTop;
            list.scrollTop = before + speed;
            if (list.scrollTop !== before) updateDropIndex(session.lastY);
        }
        session.raf = requestAnimationFrame(autoScrollStep);
    };

    const handleDragPointerDown = (e: PointerEvent, index: number) => {
        if (e.pointerType === 'mouse' && e.button !== 0) return;
        if (dragRef.current) return;
        e.preventDefault();

        const handle = e.currentTarget as HTMLElement;
        try {
            handle.setPointerCapture(e.pointerId);
        } catch {
            // Some browsers throw if the pointer is already gone; dragging still works via bubbling
        }

        const list = listRef.current;
        const row = handle.closest<HTMLElement>('[data-queue-row]');
        const session: DragSession = {
            pointerId: e.pointerId,
            from: index,
            to: index,
            lastY: e.clientY,
            rowHeight: row?.offsetHeight ?? 0,
            bottomCover: list ? Number.parseFloat(getComputedStyle(list).paddingBottom) || 0 : 0,
            raf: null,
        };
        dragRef.current = session;
        setSelectedTlid(null);
        setMenuOpen(false);
        setDraggedIndex(index);
        setDropIndex(index);
        setDragRowHeight(session.rowHeight);
        session.raf = requestAnimationFrame(autoScrollStep);
    };

    const handleDragPointerMove = (e: PointerEvent) => {
        const session = dragRef.current;
        if (!session || e.pointerId !== session.pointerId) return;
        session.lastY = e.clientY;
        positionGhost(e.clientY);
        updateDropIndex(e.clientY);
    };

    const finishDrag = (commit: boolean) => {
        const session = dragRef.current;
        if (!session) return;
        dragRef.current = null;
        if (session.raf) cancelAnimationFrame(session.raf);
        setDraggedIndex(null);
        setDropIndex(null);
        if (!commit) return;

        const { from, to } = session;
        if (to === from || to === from + 1) return; // dropped back where it was
        moveTrack(from, finalIndex(from, to));
    };

    /** Keyboard alternative to dragging: arrow keys nudge the track one step */
    const handleHandleKeyDown = (e: KeyboardEvent, index: number) => {
        if (e.key === 'ArrowUp' && index > 0) {
            e.preventDefault();
            moveTrack(index, index - 1);
        } else if (e.key === 'ArrowDown' && index < queueTracks.length - 1) {
            e.preventDefault();
            moveTrack(index, index + 1);
        }
    };

    /**
     * While dragging, rows slide to preview the order that a drop would produce:
     * the dragged row moves to the gap, the rows in between shift by one row height.
     */
    const rowShift = (index: number): number => {
        if (draggedIndex === null || dropIndex === null || dragRowHeight === 0) return 0;
        if (index === draggedIndex)
            return (finalIndex(draggedIndex, dropIndex) - draggedIndex) * dragRowHeight;
        if (dropIndex > draggedIndex && index > draggedIndex && index < dropIndex)
            return -dragRowHeight;
        if (dropIndex <= draggedIndex && index >= dropIndex && index < draggedIndex)
            return dragRowHeight;
        return 0;
    };

    const isDragging = draggedIndex !== null;
    const totalDuration = queueTracks.reduce((sum, item) => sum + (item.track.duration || 0), 0);
    const draggedItem = draggedIndex !== null ? queueTracks[draggedIndex] : undefined;

    if (loading && queueTracks.length === 0) {
        return (
            <div className="flex flex-col h-full overflow-hidden">
                <div className="flex items-center justify-center min-h-[50vh] text-fg-secondary">
                    Loading queue...
                </div>
            </div>
        );
    }

    if (queueTracks.length === 0) {
        return (
            <div className="flex flex-col h-full overflow-hidden">
                <div className="flex flex-col items-center justify-center min-h-[50vh] gap-2 text-fg-secondary text-center px-8">
                    <p>Queue is empty</p>
                    <p className="text-sm text-fg-tertiary">Add tracks from Library or Search</p>
                    <button
                        type="button"
                        className="flex items-center gap-2 mt-4 px-4 py-2 bg-bg-secondary border border-border-primary text-fg-secondary font-mono text-sm cursor-pointer transition-all duration-150 hover:text-accent-primary hover:border-accent-primary"
                        onClick={loadQueue}
                    >
                        <RefreshCw size={16} />
                        Refresh
                    </button>
                </div>
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full overflow-hidden">
            {/* Queue header with actions menu */}
            <div className="flex items-center justify-between px-4 py-1.5 border-b border-border-primary shrink-0 bg-bg-secondary">
                <span className="text-fg-secondary text-sm">
                    {queueTracks.length} tracks · {formatTotalDuration(totalDuration)}
                </span>
                <div className="relative" ref={menuRef}>
                    <button
                        type="button"
                        className="flex items-center justify-center w-8 h-8 bg-transparent border-none text-fg-secondary cursor-pointer transition-colors duration-150 hover:text-fg-primary"
                        onClick={() => setMenuOpen(!menuOpen)}
                        aria-label="Queue actions"
                        aria-expanded={menuOpen}
                    >
                        <MoreVertical size={18} />
                    </button>
                    {menuOpen && (
                        <div className="absolute right-0 top-full mt-1 min-w-40 bg-bg-secondary border border-border-primary z-50 shadow-lg">
                            <button
                                type="button"
                                className="flex items-center gap-2 w-full px-3 py-2.5 bg-transparent border-none text-fg-primary text-sm font-mono text-left cursor-pointer transition-colors duration-150 hover:bg-bg-tertiary"
                                onClick={handleShuffle}
                            >
                                <Shuffle size={15} />
                                Shuffle
                            </button>
                            <button
                                type="button"
                                className="flex items-center gap-2 w-full px-3 py-2.5 bg-transparent border-none text-error text-sm font-mono text-left cursor-pointer transition-colors duration-150 hover:bg-bg-tertiary"
                                onClick={handleClearQueue}
                            >
                                <Trash2 size={15} />
                                Clear Queue
                            </button>
                        </div>
                    )}
                </div>
            </div>

            <div
                ref={listRef}
                className={`relative flex-1 overflow-y-auto overscroll-y-contain pb-[var(--mini-player-height)] ${isDragging ? 'select-none' : ''}`}
            >
                {queueTracks.map((item, index) => {
                    const isCurrentTrack =
                        item.tlid === currentTlid || item.track.uri === current?.uri;
                    const isSelected = selectedTlid === item.tlid;
                    const isDragged = draggedIndex === index;
                    const shift = rowShift(index);

                    return (
                        <div
                            key={item.tlid}
                            data-queue-row
                            ref={isCurrentTrack ? currentTrackRef : null}
                            className={`relative ${isDragging ? 'queue-row-sliding' : ''} ${isCurrentTrack ? 'bg-bg-secondary border-l-3 border-l-accent-primary' : ''} ${isDragged ? 'queue-row-dragged' : ''}`}
                            style={
                                shift !== 0 ? { transform: `translateY(${shift}px)` } : undefined
                            }
                        >
                            <TrackItem
                                track={item.track}
                                className={`cursor-pointer touch-manipulation ${isCurrentTrack ? 'bg-bg-secondary' : ''}`}
                                leftContent={
                                    <div className="flex items-center justify-center w-10 h-10 text-fg-tertiary shrink-0">
                                        {isCurrentTrack ? (
                                            <span className="playing-indicator flex items-end justify-center gap-0.5 h-4">
                                                <span />
                                                <span />
                                                <span />
                                            </span>
                                        ) : (
                                            <span className="text-sm font-mono">{index + 1}</span>
                                        )}
                                    </div>
                                }
                                rightContent={
                                    // biome-ignore lint/a11y/useSemanticElements: a <button> cannot receive pointer capture consistently across mobile browsers
                                    <div
                                        className="flex items-center justify-center w-12 h-12 -mr-2 text-fg-tertiary shrink-0 cursor-grab touch-none select-none hover:text-fg-secondary active:cursor-grabbing"
                                        role="button"
                                        tabIndex={0}
                                        aria-label={`Reorder ${item.track.name} (drag, or use arrow keys)`}
                                        onPointerDown={(e) => handleDragPointerDown(e, index)}
                                        onPointerMove={handleDragPointerMove}
                                        onPointerUp={() => finishDrag(true)}
                                        onPointerCancel={() => finishDrag(false)}
                                        onLostPointerCapture={() => finishDrag(false)}
                                        onKeyDown={(e) => handleHandleKeyDown(e, index)}
                                        onClick={(e) => e.stopPropagation()}
                                        onContextMenu={(e) => e.preventDefault()}
                                    >
                                        <GripVertical size={20} />
                                    </div>
                                }
                                onClick={() => handleTrackTap(item.tlid)}
                            />
                            {isSelected && (
                                // biome-ignore lint/a11y/noStaticElementInteractions: only stops the outside-click handler; the buttons inside are the controls
                                // biome-ignore lint/a11y/useKeyWithClickEvents: see above
                                <div
                                    className="absolute inset-0 flex gap-px bg-border-primary"
                                    onClick={(e) => e.stopPropagation()}
                                >
                                    <button
                                        type="button"
                                        className="flex-1 flex items-center justify-center gap-1 bg-bg-secondary border-none text-fg-primary font-mono text-sm cursor-pointer transition-all duration-150 hover:bg-bg-tertiary hover:text-accent-primary"
                                        onClick={() => {
                                            handlePlayTrack(item.tlid);
                                            setSelectedTlid(null);
                                        }}
                                    >
                                        <Play size={18} />
                                        Play
                                    </button>
                                    <button
                                        type="button"
                                        className="flex-1 flex items-center justify-center gap-1 bg-bg-secondary border-none text-fg-primary font-mono text-sm cursor-pointer transition-all duration-150 hover:bg-bg-tertiary hover:text-error"
                                        onClick={() => handleRemoveTrack(item.tlid)}
                                    >
                                        <Trash2 size={18} />
                                        Remove
                                    </button>
                                </div>
                            )}
                        </div>
                    );
                })}
            </div>

            {draggedItem && (
                <div
                    ref={ghostRef}
                    className="drag-ghost"
                    style={{ transform: 'translateY(-9999px)' }}
                >
                    <div className="track-item drag-ghost-card bg-bg-tertiary border border-accent-primary shadow-lg">
                        <div className="flex-1 min-w-0 flex flex-col">
                            <div className="text-base text-fg-primary truncate leading-tight">
                                {draggedItem.track.name}
                            </div>
                            <div className="text-sm text-fg-secondary truncate leading-tight">
                                {draggedItem.track.artists?.map((a) => a.name).join(', ') ||
                                    'Unknown Artist'}
                            </div>
                        </div>
                        <GripVertical size={20} className="text-accent-primary shrink-0" />
                    </div>
                </div>
            )}
        </div>
    );
}
