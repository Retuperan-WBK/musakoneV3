/**
 * State for the Mix page. Recommendations are cached per scope so switching
 * tabs does not re-run the (slow) Tidal-backed recommender.
 */
import { atom } from 'nanostores';
import type {
    AutoplaySettings,
    AutoplayStatus,
    Insights,
    PicksScope,
    Recommendation,
} from '../services/mix';
import * as mixApi from '../services/mix';

export type MixTab = 'picks' | 'autoplay' | 'stats';

export const mixTab = atom<MixTab>('picks');
export const picksScope = atom<PicksScope>('room');
export const picks = atom<Record<PicksScope, Recommendation[] | null>>({ me: null, room: null });
export const picksLoading = atom(false);
export const picksError = atom<string | null>(null);
export const autoplayStatus = atom<AutoplayStatus | null>(null);
export const insights = atom<Insights | null>(null);
export const insightsLoading = atom(false);

let picksRequest = 0;

export async function loadPicks(scope: PicksScope, force = false): Promise<void> {
    if (!force && picks.get()[scope] !== null) return;
    const seq = ++picksRequest;
    picksLoading.set(true);
    picksError.set(null);
    try {
        const { items } = await mixApi.getRecommendations(scope);
        if (seq !== picksRequest) return;
        picks.set({ ...picks.get(), [scope]: items });
    } catch (err) {
        if (seq !== picksRequest) return;
        picksError.set(err instanceof Error ? err.message : 'Could not load recommendations');
    } finally {
        if (seq === picksRequest) picksLoading.set(false);
    }
}

/** Drop a recommendation from the list once it has been queued */
export function removePick(scope: PicksScope, uri: string): void {
    const current = picks.get();
    const list = current[scope];
    if (!list) return;
    picks.set({ ...current, [scope]: list.filter((r) => r.uri !== uri) });
}

export async function loadAutoplay(): Promise<void> {
    autoplayStatus.set(await mixApi.getAutoplay());
}

export async function saveAutoplay(patch: Partial<AutoplaySettings>): Promise<void> {
    const current = autoplayStatus.get();
    // Optimistic so the controls feel instant; the response is authoritative
    if (current) {
        autoplayStatus.set({ ...current, settings: { ...current.settings, ...patch } });
    }
    autoplayStatus.set(await mixApi.updateAutoplay(patch));
}

export async function loadInsights(): Promise<void> {
    insightsLoading.set(true);
    try {
        const data = await mixApi.getInsights();
        insights.set(data);
        autoplayStatus.set(data.autoplay);
    } finally {
        insightsLoading.set(false);
    }
}
