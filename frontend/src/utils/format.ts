/**
 * Format duration in milliseconds to human-readable time string (M:SS)
 *
 * @param ms - Duration in milliseconds
 * @returns Formatted time string (e.g., "3:45", "0:00")
 */
export function formatDuration(ms: number): string {
    if (!ms || ms < 0) return '0:00';
    const seconds = Math.floor(ms / 1000);
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
}

/**
 * Relative time for activity lists ("just now", "5 min ago", "3 h ago", "2 d ago")
 */
export function formatRelative(timestampMs: number): string {
    const seconds = Math.max(0, Math.round((Date.now() - timestampMs) / 1000));
    if (seconds < 45) return 'just now';
    const minutes = Math.round(seconds / 60);
    if (minutes < 60) return `${minutes} min ago`;
    const hours = Math.round(minutes / 60);
    if (hours < 24) return `${hours} h ago`;
    const days = Math.round(hours / 24);
    return `${days} d ago`;
}
