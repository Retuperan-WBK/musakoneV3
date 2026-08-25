import { useStore } from '@nanostores/preact';
import { Pause, Play, SkipBack, SkipForward, Volume2 } from 'lucide-preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import * as mopidy from '../services/mopidy';
import {
    currentTrack,
    isPlaying,
    timePosition,
    updatePlaybackState,
    volume,
} from '../stores/player';
import { triggerScrollToCurrent } from '../stores/queue';
import { formatDuration } from '../utils/format';

/** How often to sync with backend to correct drift (in seconds) */
const BACKEND_SYNC_INTERVAL_SECONDS = 10;

/** Threshold in ms - if position is below this, previous goes to previous track */
const PREVIOUS_THRESHOLD_MS = 3000;

/** The volume popup closes by itself after this much time without adjustments */
const VOLUME_AUTO_CLOSE_MS = 4000;

export function MiniPlayer() {
    const track = useStore(currentTrack);
    const playing = useStore(isPlaying);
    const position = useStore(timePosition);
    const currentVolume = useStore(volume);
    const [volumeOpen, setVolumeOpen] = useState(false);
    const localUpdateInterval = useRef<number | null>(null);
    const lastSyncTime = useRef<number>(Date.now());
    const lastSyncPosition = useRef<number>(position);
    const volumePopupRef = useRef<HTMLDivElement>(null);
    const volumeDebounceRef = useRef<number | null>(null);

    // Update time position while playing - derive locally, sync periodically
    useEffect(() => {
        if (playing && track) {
            lastSyncTime.current = Date.now();
            lastSyncPosition.current = position;

            localUpdateInterval.current = window.setInterval(async () => {
                const now = Date.now();
                const elapsed = now - lastSyncTime.current;
                const secondsSinceSync = elapsed / 1000;

                if (secondsSinceSync >= BACKEND_SYNC_INTERVAL_SECONDS) {
                    try {
                        const pos = await mopidy.getTimePosition();
                        updatePlaybackState({ timePosition: pos });
                        lastSyncTime.current = now;
                        lastSyncPosition.current = pos;
                    } catch {
                        const derivedPosition = lastSyncPosition.current + elapsed;
                        updatePlaybackState({ timePosition: derivedPosition });
                    }
                } else {
                    const derivedPosition = lastSyncPosition.current + elapsed;
                    updatePlaybackState({ timePosition: derivedPosition });
                }
            }, 100);
        }

        return () => {
            if (localUpdateInterval.current) {
                clearInterval(localUpdateInterval.current);
                localUpdateInterval.current = null;
            }
        };
    }, [playing, track]);

    // Close volume popup when clicking outside
    useEffect(() => {
        const handleClickOutside = (event: Event) => {
            if (
                volumeOpen &&
                volumePopupRef.current &&
                !volumePopupRef.current.contains(event.target as Node)
            ) {
                setVolumeOpen(false);
            }
        };

        if (volumeOpen) {
            document.addEventListener('mousedown', handleClickOutside);
            document.addEventListener('touchstart', handleClickOutside);
        }
        const autoClose = volumeOpen
            ? window.setTimeout(() => setVolumeOpen(false), VOLUME_AUTO_CLOSE_MS)
            : null;

        return () => {
            if (autoClose !== null) clearTimeout(autoClose);
            document.removeEventListener('mousedown', handleClickOutside);
            document.removeEventListener('touchstart', handleClickOutside);
        };
    }, [volumeOpen, currentVolume]);

    const handlePlayPause = async () => {
        try {
            if (playing) {
                await mopidy.pause();
            } else {
                await mopidy.resume();
            }
        } catch (err) {
            console.error('Failed to toggle playback:', err);
        }
    };

    const handleNext = async () => {
        try {
            await mopidy.next();
        } catch (err) {
            console.error('Failed to skip to next:', err);
        }
    };

    const handlePrevious = async () => {
        try {
            if (position <= PREVIOUS_THRESHOLD_MS) {
                await mopidy.previous();
            } else {
                await mopidy.seek(0);
                updatePlaybackState({ timePosition: 0 });
                lastSyncTime.current = Date.now();
                lastSyncPosition.current = 0;
            }
        } catch (err) {
            console.error('Failed to skip to previous:', err);
        }
    };

    const handleSeek = async (e: Event) => {
        const input = e.target as HTMLInputElement;
        const newPosition = parseInt(input.value, 10);
        try {
            await mopidy.seek(newPosition);
            updatePlaybackState({ timePosition: newPosition });
            lastSyncTime.current = Date.now();
            lastSyncPosition.current = newPosition;
        } catch (err) {
            console.error('Failed to seek:', err);
        }
    };

    const handleVolumeChange = (e: Event) => {
        const input = e.target as HTMLInputElement;
        const newVolume = parseInt(input.value, 10);
        updatePlaybackState({ volume: newVolume });
        if (volumeDebounceRef.current) {
            clearTimeout(volumeDebounceRef.current);
        }
        volumeDebounceRef.current = window.setTimeout(async () => {
            try {
                await mopidy.setVolume(newVolume);
            } catch (err) {
                console.error('Failed to set volume:', err);
            }
        }, 150);
    };

    const progress = track?.duration ? (position / track.duration) * 100 : 0;

    const handleTrackClick = () => {
        if (!track) return;
        setTimeout(() => {
            triggerScrollToCurrent();
        }, 100);
    };

    return (
        <div
            className="w-full bg-bg-tertiary border-t border-border-primary flex flex-col"
            style={{ height: 'var(--mini-player-height)', flexShrink: 0 }}
        >
            {/* Progress bar */}
            <div className="progress-bar" style={{ '--progress': `${progress}%` }}>
                {track && (
                    <input
                        type="range"
                        className="absolute -top-1.5 left-0 w-full h-4 m-0 opacity-0 cursor-pointer"
                        style={{ zIndex: 0, pointerEvents: 'none' }}
                        min={0}
                        max={track.duration || 100}
                        value={position}
                        onChange={handleSeek}
                        aria-label="Seek"
                    />
                )}
            </div>

            <div className="flex items-center px-2 py-1 gap-1">
                <button
                    type="button"
                    className={`flex-1 min-w-0 flex flex-col items-start gap-0.5 bg-transparent border-none p-0 text-left font-mono ${track ? 'cursor-pointer select-none active:opacity-70' : 'cursor-default'}`}
                    onClick={handleTrackClick}
                    disabled={!track}
                    aria-label="Show current track in queue"
                >
                    {track ? (
                        <>
                            <div className="w-full text-base text-fg-primary truncate">
                                {track.name}
                            </div>
                            <div className="w-full text-sm text-fg-secondary truncate">
                                {track.artists?.map((a) => a.name).join(', ') || 'Unknown Artist'}
                            </div>
                        </>
                    ) : (
                        <div className="text-fg-tertiary italic">No track playing</div>
                    )}
                </button>

                {track && (
                    <div className="text-xs text-fg-tertiary whitespace-nowrap shrink-0">
                        {formatDuration(position)} / {formatDuration(track.duration)}
                    </div>
                )}

                <div className="flex gap-1 shrink-0">
                    <button
                        type="button"
                        className="btn-control"
                        onClick={handlePrevious}
                        disabled={!track}
                        aria-label="Previous track"
                    >
                        <SkipBack size={20} />
                    </button>

                    <button
                        type="button"
                        className={`btn-control ${playing ? '' : ''} bg-accent-primary text-bg-primary border-accent-primary hover:brightness-110 active:brightness-90`}
                        onClick={handlePlayPause}
                        disabled={!track}
                        aria-label={playing ? 'Pause' : 'Play'}
                    >
                        {playing ? <Pause size={20} /> : <Play size={20} />}
                    </button>

                    <button
                        type="button"
                        className="btn-control"
                        onClick={handleNext}
                        disabled={!track}
                        aria-label="Next track"
                    >
                        <SkipForward size={20} />
                    </button>
                </div>

                <div
                    className="relative flex items-center shrink-0"
                    ref={volumePopupRef}
                    style={{ zIndex: 200 }}
                >
                    <button
                        type="button"
                        className={`btn-control ${volumeOpen ? 'border-accent-primary' : ''}`}
                        onClick={() => setVolumeOpen(!volumeOpen)}
                        onKeyDown={(e) => {
                            if (e.key === 'Escape' && volumeOpen) {
                                setVolumeOpen(false);
                            }
                        }}
                        aria-label="Volume control"
                        aria-expanded={volumeOpen}
                    >
                        <Volume2 size={20} />
                    </button>

                    {volumeOpen && (
                        <div
                            className="absolute bottom-[calc(100%+0.5rem)] right-0 bg-bg-tertiary border border-border-primary rounded-lg py-2 flex flex-col items-center gap-2 z-100 shadow-lg"
                            style={{ zIndex: 202 }}
                        >
                            <input
                                type="range"
                                className="volume-slider"
                                min={0}
                                max={100}
                                value={currentVolume}
                                onChange={handleVolumeChange}
                                aria-label="Volume"
                            />
                            <div className="text-xs text-fg-secondary min-w-10 text-center">
                                {currentVolume}%
                            </div>
                        </div>
                    )}
                </div>
            </div>
        </div>
    );
}
