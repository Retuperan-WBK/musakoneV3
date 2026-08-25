import { Check, ListPlus, Plus } from 'lucide-preact';
import type { JSX } from 'preact';
import { openAddToPlaylistModal } from '../stores/playlists';
import { SwipeableItem } from './SwipeableItem';
import { DefaultTrackIcon, TrackItem, type TrackItemData } from './TrackItem';

export interface SwipeableTrackItemProps {
    /** Track data to display */
    track: TrackItemData;
    /** Track URI for add-to-playlist */
    trackUri?: string;
    /** Whether the track is already in the queue */
    isQueued: boolean;
    /** Whether interactions are disabled */
    disabled?: boolean;
    /** Callback when track should be added to end of queue */
    onAdd: () => void;
    /** Callback when track should be added next in queue */
    onAddNext: () => void;
    /** Optional icon to display (defaults to music icon) */
    icon?: JSX.Element;
    /** Show album name in metadata */
    showAlbum?: boolean;
    /** Show duration */
    showDuration?: boolean;
    /** Custom metadata display (overrides artist/album) */
    customMeta?: string;
    /** Label for swipe left action */
    leftLabel?: string;
    /** Label for swipe right action */
    rightLabel?: string;
    /** Swipe threshold in pixels */
    threshold?: number;
}

/**
 * Reusable swipeable track item component with add-to-queue functionality.
 * Used across Library and Search views with consistent styling.
 */
export function SwipeableTrackItem({
    track,
    trackUri,
    isQueued,
    disabled = false,
    onAdd,
    onAddNext,
    icon = DefaultTrackIcon,
    showAlbum = false,
    showDuration = true,
    customMeta,
    leftLabel = '+ Queue Next',
    rightLabel = '+ Queue',
    threshold = 80,
}: SwipeableTrackItemProps) {
    return (
        <SwipeableItem
            isDisabled={isQueued || disabled}
            onSwipeLeft={onAddNext}
            onSwipeRight={onAdd}
            leftLabel={leftLabel}
            rightLabel={rightLabel}
            threshold={threshold}
            className="flex items-center bg-bg-primary min-h-12 w-full relative z-1 transition-transform duration-100"
        >
            <TrackItem
                track={track}
                icon={icon}
                showAlbum={showAlbum}
                showDuration={showDuration}
                customMeta={customMeta}
                rightContent={
                    <>
                        {trackUri && (
                            <button
                                type="button"
                                className="flex items-center justify-center w-8 h-8 bg-transparent border border-border-primary text-fg-tertiary cursor-pointer shrink-0 transition-all duration-150 hover:text-accent-primary hover:border-accent-primary active:bg-bg-secondary"
                                onClick={(e) => {
                                    e.stopPropagation();
                                    openAddToPlaylistModal(trackUri);
                                }}
                                aria-label={`Add ${track.name} to playlist`}
                                title="Add to playlist"
                            >
                                <ListPlus size={18} />
                            </button>
                        )}
                        {isQueued ? (
                            <div
                                className="flex items-center justify-center w-8 h-8 text-success shrink-0"
                                title="Already in queue"
                            >
                                <Check size={18} />
                            </div>
                        ) : (
                            <button
                                type="button"
                                className="flex items-center justify-center w-8 h-8 bg-transparent border border-border-primary text-fg-tertiary cursor-pointer shrink-0 transition-all duration-150 hover:text-accent-primary hover:border-accent-primary active:bg-bg-secondary"
                                onClick={(e) => {
                                    e.stopPropagation();
                                    onAdd();
                                }}
                                aria-label={`Add ${track.name} to queue`}
                                title="Add to queue"
                            >
                                <Plus size={18} />
                            </button>
                        )}
                    </>
                }
            />
        </SwipeableItem>
    );
}
