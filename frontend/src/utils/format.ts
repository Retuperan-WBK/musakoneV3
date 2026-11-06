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
