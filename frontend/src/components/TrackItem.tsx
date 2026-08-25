import { Music } from 'lucide-preact';
import type { JSX } from 'preact';
import { formatDuration } from '../utils/format';

export interface TrackItemData {
    name: string;
    duration?: number;
    artists?: Array<{ name: string }>;
    album?: { name: string } | null;
}

interface TrackItemProps {
    /** Track data to display */
    track: TrackItemData;
    /** Optional icon to display (defaults to Music icon) */
    icon?: JSX.Element;
    /** Optional left content (e.g., index or playing indicator) */
    leftContent?: JSX.Element;
    /** Optional right content (e.g., buttons or status) */
    rightContent?: JSX.Element;
    /** Additional CSS classes */
    className?: string;
    /** Show album name in metadata */
    showAlbum?: boolean;
    /** Show duration */
    showDuration?: boolean;
    /** Custom metadata display */
    customMeta?: string;
    /** Click handler */
    onClick?: () => void;
    /** Double click handler */
    onDoubleClick?: () => void;
}

/**
 * Reusable track item component for displaying track information
 * Used across Queue, Library, and Search views with different configurations
 */
export function TrackItem({
    track,
    icon,
    leftContent,
    rightContent,
    className = '',
    showAlbum = false,
    showDuration = true,
    customMeta,
    onClick,
    onDoubleClick,
}: TrackItemProps) {
    const artistNames = track.artists?.map((a) => a.name).join(', ') || 'Unknown Artist';
    const metadata =
        customMeta ||
        (showAlbum && track.album ? `${artistNames} • ${track.album.name}` : artistNames);

    return (
        // biome-ignore lint/a11y/noStaticElementInteractions: role/tabIndex/keyboard are set whenever onClick is given
        <div
            className={`track-item ${className}`}
            role={onClick ? 'button' : undefined}
            tabIndex={onClick ? 0 : undefined}
            onClick={onClick}
            onDblClick={onDoubleClick}
            onKeyDown={
                onClick
                    ? (e) => {
                          if (e.key === 'Enter' || e.key === ' ') {
                              e.preventDefault();
                              onClick();
                          }
                      }
                    : undefined
            }
        >
            {leftContent && <div className="flex items-center shrink-0">{leftContent}</div>}

            {icon && (
                <div className="flex items-center justify-center text-fg-secondary shrink-0">
                    {icon}
                </div>
            )}

            <div className="flex-1 min-w-0 flex flex-col gap-0">
                <div className="text-base font-medium text-fg-primary truncate leading-tight">
                    {track.name}
                </div>
                <div className="text-sm text-fg-secondary truncate leading-tight">{metadata}</div>
            </div>

            {showDuration && track.duration !== undefined && (
                <div className="font-mono text-sm text-fg-secondary shrink-0 min-w-10 text-right">
                    {formatDuration(track.duration)}
                </div>
            )}

            {rightContent && <div className="flex items-center gap-1 shrink-0">{rightContent}</div>}
        </div>
    );
}

/**
 * Default icon for track items
 */
export const DefaultTrackIcon = <Music size={20} />;
