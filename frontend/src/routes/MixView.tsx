import { useStore } from '@nanostores/preact';
import { BarChart3, ListPlus, Minus, Plus, RefreshCw, Sparkles, Zap } from 'lucide-preact';
import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { SwipeableTrackItem } from '../components/SwipeableTrackItem';
import { useAddToQueue } from '../hooks/useAddToQueue';
import type { AutoplaySettings, Insights, PicksScope, Recommendation } from '../services/mix';
import { fillAutoplay } from '../services/mix';
import { currentUser } from '../stores/auth';
import {
    autoplayStatus,
    insights,
    insightsLoading,
    loadAutoplay,
    loadInsights,
    loadPicks,
    type MixTab,
    mixTab,
    picks,
    picksError,
    picksLoading,
    picksScope,
    removePick,
    saveAutoplay,
} from '../stores/mix';
import { queue } from '../stores/queue';
import { showToast, toastError } from '../stores/toast';
import { formatRelative } from '../utils/format';

const TABS: Array<{ id: MixTab; label: string; icon: typeof Sparkles }> = [
    { id: 'picks', label: 'Picks', icon: Sparkles },
    { id: 'autoplay', label: 'Autoplay', icon: Zap },
    { id: 'stats', label: 'Stats', icon: BarChart3 },
];

const TAB_CLASS =
    'flex-1 flex items-center justify-center gap-1 min-h-12 bg-transparent border-none border-b-2 border-b-transparent font-mono text-sm cursor-pointer transition-colors duration-150';
const PILL_CLASS =
    'min-h-10 px-3 font-mono text-sm border cursor-pointer transition-colors duration-150 active:opacity-70';
const BTN_CLASS =
    'flex items-center justify-center gap-2 min-h-11 px-4 font-mono text-sm border cursor-pointer transition-colors duration-150 disabled:opacity-40 disabled:cursor-not-allowed active:opacity-70';

export function MixView() {
    const tab = useStore(mixTab);

    return (
        <div className="flex flex-col h-full overflow-hidden">
            <div className="flex border-b border-border-primary shrink-0 bg-bg-secondary">
                {TABS.map(({ id, label, icon: Icon }) => (
                    <button
                        type="button"
                        key={id}
                        className={`${TAB_CLASS} ${tab === id ? 'text-accent-primary border-b-accent-primary' : 'text-fg-secondary hover:text-fg-primary'}`}
                        onClick={() => mixTab.set(id)}
                        aria-current={tab === id ? 'page' : undefined}
                    >
                        <Icon size={16} />
                        {label}
                    </button>
                ))}
            </div>
            {tab === 'picks' && <PicksTab />}
            {tab === 'autoplay' && <AutoplayTab />}
            {tab === 'stats' && <StatsTab />}
        </div>
    );
}

// ─── Picks ──────────────────────────────────────────────────────────

function PicksTab() {
    const scope = useStore(picksScope);
    const all = useStore(picks);
    const loading = useStore(picksLoading);
    const error = useStore(picksError);
    const queueTracks = useStore(queue);
    const { addToQueue, addNext } = useAddToQueue();
    const items = all[scope];

    useEffect(() => {
        loadPicks(scope);
    }, [scope]);

    const queuedUris = useMemo(() => new Set(queueTracks.map((t) => t.track.uri)), [queueTracks]);

    const queueOne = async (rec: Recommendation, next = false) => {
        try {
            if (next) {
                await addNext(rec.uri);
            } else {
                await addToQueue(rec.uri);
            }
            removePick(scope, rec.uri);
            showToast(`Queued ${rec.name}`, 'success');
        } catch {
            toastError(`Could not queue ${rec.name}`);
        }
    };

    const queueBatch = async () => {
        const batch = (items ?? []).filter((r) => !queuedUris.has(r.uri)).slice(0, 5);
        if (batch.length === 0) return;
        try {
            await addToQueue(batch.map((r) => r.uri));
            for (const r of batch) removePick(scope, r.uri);
            showToast(`Queued ${batch.length} tracks`, 'success');
        } catch {
            toastError('Could not queue tracks');
        }
    };

    return (
        <>
            <div className="flex items-center gap-2 px-3 py-2 border-b border-border-primary shrink-0">
                <div className="flex gap-px flex-1">
                    {(['room', 'me'] as PicksScope[]).map((s) => (
                        <button
                            type="button"
                            key={s}
                            className={`${PILL_CLASS} flex-1 ${scope === s ? 'bg-accent-primary border-accent-primary text-fg-primary' : 'bg-transparent border-border-primary text-fg-secondary'}`}
                            onClick={() => picksScope.set(s)}
                            aria-pressed={scope === s}
                        >
                            {s === 'room' ? 'For the room' : 'For me'}
                        </button>
                    ))}
                </div>
                <button
                    type="button"
                    className={`${BTN_CLASS} bg-transparent border-border-primary text-fg-secondary min-w-11 px-0`}
                    onClick={() => loadPicks(scope, true)}
                    disabled={loading}
                    aria-label="Refresh recommendations"
                >
                    <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
                </button>
                <button
                    type="button"
                    className={`${BTN_CLASS} bg-transparent border-border-primary text-fg-primary`}
                    onClick={queueBatch}
                    disabled={loading || !items || items.length === 0}
                >
                    <ListPlus size={16} />
                    Queue 5
                </button>
            </div>

            {loading && !items ? (
                <Status
                    text={`Digging through Tidal for ${scope === 'room' ? 'the room' : 'you'}…`}
                    pulse
                />
            ) : error && !items ? (
                <Status text={error} error onRetry={() => loadPicks(scope, true)} />
            ) : !items || items.length === 0 ? (
                <Status text="Nothing to suggest yet – play and queue some music first, the picks learn from that." />
            ) : (
                <div
                    className={`flex-1 overflow-y-auto overscroll-y-contain pb-2 ${loading ? 'opacity-60' : ''}`}
                >
                    {items.map((rec) => (
                        <SwipeableTrackItem
                            key={rec.uri}
                            track={{
                                name: rec.name,
                                duration: rec.duration_ms ?? undefined,
                                artists: [{ name: rec.artist }],
                            }}
                            trackUri={rec.uri}
                            isQueued={queuedUris.has(rec.uri)}
                            onAdd={() => queueOne(rec)}
                            onAddNext={() => queueOne(rec, true)}
                            customMeta={`${rec.artist} · ${rec.reasons.join(' · ')}`}
                            leftLabel="+ Queue Next"
                            rightLabel="+ Queue"
                        />
                    ))}
                </div>
            )}
        </>
    );
}

// ─── Autoplay ───────────────────────────────────────────────────────

function AutoplayTab() {
    const status = useStore(autoplayStatus);
    const user = useStore(currentUser);
    const [filling, setFilling] = useState(false);
    const discoveryTimer = useRef<number | null>(null);

    useEffect(() => {
        if (!status) loadAutoplay().catch(() => toastError('Could not load autoplay settings'));
    }, [status]);

    const save = async (patch: Partial<AutoplaySettings>) => {
        try {
            await saveAutoplay(patch);
        } catch {
            toastError('Could not save autoplay settings');
            loadAutoplay().catch(() => {});
        }
    };

    const handleFill = async () => {
        setFilling(true);
        try {
            const { added } = await fillAutoplay();
            showToast(`Autoplay added ${added} tracks`, 'success');
            loadAutoplay().catch(() => {});
        } catch (err) {
            toastError(err instanceof Error ? err.message : 'Autoplay could not fill the queue');
        } finally {
            setFilling(false);
        }
    };

    if (!status) return <Status text="Loading autoplay…" pulse />;
    const s = status.settings;
    const myMode = user ? `user:${user.id}` : 'user:0';

    return (
        <div className="flex-1 overflow-y-auto overscroll-y-contain px-4 py-3 flex flex-col gap-5">
            <section className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                    <div className="text-base text-fg-primary">Autoplay</div>
                    <div className="text-sm text-fg-secondary">
                        {s.enabled
                            ? `On · added ${status.added_24h} tracks in the last 24 h`
                            : 'Off · the queue stops when it runs out'}
                    </div>
                </div>
                <button
                    type="button"
                    role="switch"
                    aria-checked={s.enabled}
                    aria-label="Autoplay"
                    className={`relative shrink-0 w-14 h-8 border cursor-pointer transition-colors duration-150 ${s.enabled ? 'bg-accent-primary border-accent-primary' : 'bg-bg-tertiary border-border-primary'}`}
                    onClick={() => save({ enabled: !s.enabled })}
                >
                    <span
                        className={`absolute top-1 w-5 h-5 bg-fg-primary transition-all duration-150 ${s.enabled ? 'left-8' : 'left-1'}`}
                    />
                </button>
            </section>

            <section className="flex flex-col gap-2">
                <div className="text-sm text-fg-secondary">Whose taste</div>
                <div className="flex gap-px">
                    <button
                        type="button"
                        className={`${PILL_CLASS} flex-1 ${s.mode === 'room' ? 'bg-accent-primary border-accent-primary text-fg-primary' : 'bg-transparent border-border-primary text-fg-secondary'}`}
                        onClick={() => save({ mode: 'room' })}
                        aria-pressed={s.mode === 'room'}
                    >
                        Everyone here
                    </button>
                    <button
                        type="button"
                        className={`${PILL_CLASS} flex-1 ${s.mode === myMode ? 'bg-accent-primary border-accent-primary text-fg-primary' : 'bg-transparent border-border-primary text-fg-secondary'}`}
                        onClick={() => save({ mode: myMode })}
                        aria-pressed={s.mode === myMode}
                        disabled={!user}
                    >
                        Just me
                    </button>
                </div>
                {s.mode !== 'room' && s.mode !== myMode && (
                    <div className="text-xs text-fg-tertiary">
                        Currently following {s.mode.replace('user:', 'user #')}
                    </div>
                )}
            </section>

            <section className="flex flex-col gap-2">
                <div className="flex justify-between text-sm">
                    <span className="text-fg-secondary">Discovery</span>
                    <span className="text-fg-primary">{Math.round(s.discovery * 100)}%</span>
                </div>
                <input
                    type="range"
                    min={0}
                    max={100}
                    value={Math.round(s.discovery * 100)}
                    className="w-full h-11 accent-accent-primary"
                    aria-label="Discovery: favourites to new music"
                    onInput={(e) => {
                        const value = Number((e.target as HTMLInputElement).value) / 100;
                        const current = autoplayStatus.get();
                        if (current) {
                            autoplayStatus.set({
                                ...current,
                                settings: { ...current.settings, discovery: value },
                            });
                        }
                        if (discoveryTimer.current) clearTimeout(discoveryTimer.current);
                        discoveryTimer.current = window.setTimeout(
                            () => save({ discovery: value }),
                            400
                        );
                    }}
                />
                <div className="flex justify-between text-xs text-fg-tertiary">
                    <span>Safe favourites</span>
                    <span>New music</span>
                </div>
            </section>

            <Stepper
                label="Keep ahead"
                hint="refill when fewer tracks than this remain after the current one"
                value={s.min_ahead}
                min={0}
                max={20}
                onChange={(v) => save({ min_ahead: v })}
            />
            <Stepper
                label="Batch size"
                hint="tracks added per refill"
                value={s.batch_size}
                min={1}
                max={20}
                onChange={(v) => save({ batch_size: v })}
            />

            <button
                type="button"
                className={`${BTN_CLASS} bg-bg-tertiary border-border-primary text-fg-primary hover:border-accent-primary min-h-12`}
                onClick={handleFill}
                disabled={filling}
            >
                <Zap size={16} className={filling ? 'animate-pulse' : ''} />
                {filling ? 'Picking tracks…' : `Add ${s.batch_size} tracks now`}
            </button>
        </div>
    );
}

function Stepper({
    label,
    hint,
    value,
    min,
    max,
    onChange,
}: {
    label: string;
    hint: string;
    value: number;
    min: number;
    max: number;
    onChange: (value: number) => void;
}) {
    return (
        <section className="flex items-center justify-between gap-3">
            <div className="min-w-0">
                <div className="text-sm text-fg-primary">{label}</div>
                <div className="text-xs text-fg-tertiary">{hint}</div>
            </div>
            <div className="flex items-center shrink-0">
                <button
                    type="button"
                    className={`${BTN_CLASS} w-11 px-0 bg-transparent border-border-primary text-fg-primary`}
                    onClick={() => onChange(Math.max(min, value - 1))}
                    disabled={value <= min}
                    aria-label={`Decrease ${label}`}
                >
                    <Minus size={16} />
                </button>
                <span className="w-10 text-center text-base text-fg-primary" aria-live="polite">
                    {value}
                </span>
                <button
                    type="button"
                    className={`${BTN_CLASS} w-11 px-0 bg-transparent border-border-primary text-fg-primary`}
                    onClick={() => onChange(Math.min(max, value + 1))}
                    disabled={value >= max}
                    aria-label={`Increase ${label}`}
                >
                    <Plus size={16} />
                </button>
            </div>
        </section>
    );
}

// ─── Stats ──────────────────────────────────────────────────────────

function StatsTab() {
    const data = useStore(insights);
    const loading = useStore(insightsLoading);
    const [error, setError] = useState<string | null>(null);

    const load = () => {
        setError(null);
        loadInsights().catch((err) =>
            setError(err instanceof Error ? err.message : 'Could not load stats')
        );
    };

    useEffect(() => {
        load();
    }, []);

    if (!data) {
        return error ? (
            <Status text={error} error onRetry={load} />
        ) : (
            <Status text="Crunching numbers…" pulse />
        );
    }

    const { room, me } = data;
    const t = room.totals;
    const hourly = localHourly(room.hourly);
    const daily = lastDays(room.daily, 14);
    const maxArtist = Math.max(...room.top_artists.map((a) => a.score), 1);

    return (
        <div
            className={`flex-1 overflow-y-auto overscroll-y-contain px-4 py-3 flex flex-col gap-5 ${loading ? 'opacity-70' : ''}`}
        >
            <div className="grid grid-cols-2 gap-2">
                <Tile
                    value={t.plays.toLocaleString()}
                    label="tracks played"
                    sub={`${t.plays_24h} today`}
                />
                <Tile value={(t.listen_minutes / 60).toFixed(1)} label="hours listened" />
                <Tile
                    value={t.queue_adds.toLocaleString()}
                    label="tracks queued"
                    sub={`${t.autoplay_adds_24h} by autoplay today`}
                />
                <Tile
                    value={String(t.users)}
                    label="people"
                    sub={`${room.active_users.length} active this week`}
                />
            </div>

            {data.now_playing.track_name && (
                <Section title="Now playing">
                    <div className="text-fg-primary truncate">{data.now_playing.track_name}</div>
                    <div className="text-sm text-fg-secondary truncate">
                        {data.now_playing.artist_name ?? 'Unknown artist'} ·{' '}
                        {data.now_playing.queue_length} in queue
                    </div>
                </Section>
            )}

            <Section title="Plays by hour · last 7 days">
                <Bars
                    items={hourly.map((h) => ({
                        key: `h${h.hour}`,
                        value: h.plays,
                        label: h.hour % 6 === 0 ? String(h.hour) : '',
                    }))}
                />
            </Section>

            <Section title="Plays per day · last 14 days">
                <Bars
                    items={daily.map((d) => ({
                        key: d.day,
                        value: d.plays,
                        label: d.day.slice(8),
                    }))}
                />
            </Section>

            <Section title="Clubroom favourites">
                {room.top_tracks.length === 0 ? (
                    <Empty />
                ) : (
                    room.top_tracks.map((track, i) => (
                        <Row
                            key={track.uri}
                            rank={i + 1}
                            title={track.name ?? track.uri}
                            meta={`${track.artist ?? 'Unknown artist'} · ${track.plays} plays · ${track.users ?? 0} ${track.users === 1 ? 'person' : 'people'}`}
                        />
                    ))
                )}
            </Section>

            <Section title="Top artists">
                {room.top_artists.length === 0 ? (
                    <Empty />
                ) : (
                    room.top_artists.map((artist, i) => (
                        <div key={artist.name} className="py-1">
                            <Row
                                rank={i + 1}
                                title={artist.name}
                                meta={`${artist.plays} plays · ${artist.users ?? 0} people`}
                            />
                            <div className="h-1 bg-bg-tertiary mt-1">
                                <div
                                    className="h-full bg-accent-primary"
                                    style={{
                                        width: `${Math.max(2, (artist.score / maxArtist) * 100)}%`,
                                    }}
                                />
                            </div>
                        </div>
                    ))
                )}
            </Section>

            {room.taste_map.length > 0 && (
                <Section title="Who likes what">
                    {room.taste_map.map((entry) => (
                        <div key={entry.user_id} className="flex gap-2 py-1 text-sm">
                            <span className="text-accent-primary shrink-0">{entry.username}</span>
                            <span className="text-fg-secondary truncate">
                                {entry.artists.map((a) => a.name).join(', ')}
                            </span>
                        </div>
                    ))}
                </Section>
            )}

            <Section title="Recently queued">
                {room.recent_adds.length === 0 ? (
                    <Empty />
                ) : (
                    room.recent_adds.map((add) => (
                        <div key={`${add.timestamp_ms}-${add.username}`} className="py-1 text-sm">
                            <div className="flex justify-between gap-2">
                                <span
                                    className={
                                        add.autoplay ? 'text-fg-tertiary' : 'text-accent-primary'
                                    }
                                >
                                    {add.autoplay ? '⚡ autoplay' : add.username}
                                </span>
                                <span className="text-fg-tertiary shrink-0">
                                    {formatRelative(add.timestamp_ms)}
                                </span>
                            </div>
                            <div className="text-fg-secondary truncate">
                                {add.tracks.length > 0 ? add.tracks.join(', ') : 'tracks'}
                            </div>
                        </div>
                    ))
                )}
            </Section>

            <Section title="Recently played">
                {room.recent_plays.length === 0 ? (
                    <Empty />
                ) : (
                    room.recent_plays.map((play) => (
                        <div
                            key={`${play.timestamp_ms}-${play.uri}`}
                            className="flex justify-between gap-2 py-1 text-sm"
                        >
                            <span className="min-w-0 truncate">
                                <span className="text-fg-primary">{play.name ?? 'Unknown'}</span>
                                <span className="text-fg-secondary">
                                    {' '}
                                    · {play.artist ?? 'Unknown artist'}
                                </span>
                                {play.username && (
                                    <span className="text-fg-tertiary"> · {play.username}</span>
                                )}
                            </span>
                            <span className="text-fg-tertiary shrink-0">
                                {formatRelative(play.timestamp_ms)}
                            </span>
                        </div>
                    ))
                )}
            </Section>

            <Section title="You">
                <div className="grid grid-cols-3 gap-2 mb-2">
                    <Tile value={String(me.stats.playback ?? 0)} label="controls" small />
                    <Tile value={String(me.stats.queue ?? 0)} label="queued" small />
                    <Tile value={String(me.stats.listen_minutes ?? 0)} label="min heard" small />
                </div>
                {me.top_artists.length === 0 && me.top_tracks.length === 0 ? (
                    <div className="text-sm text-fg-tertiary">
                        Queue and play some music – your taste profile builds from what you add and
                        how long it plays.
                    </div>
                ) : (
                    <>
                        {me.top_artists.slice(0, 5).map((a, i) => (
                            <Row
                                key={a.name}
                                rank={i + 1}
                                title={a.name}
                                meta={`${a.plays} plays`}
                            />
                        ))}
                        {me.top_tracks.slice(0, 5).map((track, i) => (
                            <Row
                                key={track.uri}
                                rank={i + 1}
                                title={track.name ?? track.uri}
                                meta={`${track.artist ?? 'Unknown artist'} · ${track.my_plays ?? 0} plays`}
                            />
                        ))}
                    </>
                )}
            </Section>

            <div className="text-xs text-fg-tertiary text-center pb-2">
                Updated {formatRelative(data.generated_at)} ·{' '}
                <button
                    type="button"
                    className="bg-transparent border-none text-accent-primary cursor-pointer font-mono text-xs"
                    onClick={load}
                >
                    refresh
                </button>
            </div>
        </div>
    );
}

// ─── Shared bits ────────────────────────────────────────────────────

function Status({
    text,
    pulse,
    error,
    onRetry,
}: {
    text: string;
    pulse?: boolean;
    error?: boolean;
    onRetry?: () => void;
}) {
    return (
        <div
            className={`flex flex-col items-center justify-center min-h-[40vh] gap-3 px-8 text-center ${error ? 'text-error' : 'text-fg-secondary'}`}
        >
            <p className={pulse ? 'animate-pulse' : ''}>{text}</p>
            {onRetry && (
                <button
                    type="button"
                    className={`${BTN_CLASS} bg-transparent border-border-primary text-fg-secondary`}
                    onClick={onRetry}
                >
                    Retry
                </button>
            )}
        </div>
    );
}

function Section({ title, children }: { title: string; children: preact.ComponentChildren }) {
    return (
        <section>
            <h2 className="text-sm text-fg-tertiary uppercase tracking-wide mb-1 border-b border-border-secondary pb-1">
                {title}
            </h2>
            {children}
        </section>
    );
}

function Tile({
    value,
    label,
    sub,
    small,
}: {
    value: string;
    label: string;
    sub?: string;
    small?: boolean;
}) {
    return (
        <div className="border border-border-primary bg-bg-secondary px-3 py-2 min-w-0">
            <div className={`${small ? 'text-lg' : 'text-2xl'} text-fg-primary truncate`}>
                {value}
            </div>
            <div className="text-xs text-fg-secondary truncate">{label}</div>
            {sub && <div className="text-xs text-fg-tertiary truncate">{sub}</div>}
        </div>
    );
}

function Row({ rank, title, meta }: { rank: number; title: string; meta: string }) {
    return (
        <div className="flex items-baseline gap-2 py-0.5 min-w-0">
            <span className="text-fg-tertiary text-sm w-5 shrink-0 text-right">{rank}</span>
            <div className="min-w-0 flex-1">
                <div className="text-fg-primary truncate">{title}</div>
                <div className="text-xs text-fg-secondary truncate">{meta}</div>
            </div>
        </div>
    );
}

function Empty() {
    return <div className="text-sm text-fg-tertiary py-1">Nothing yet</div>;
}

function Bars({ items }: { items: Array<{ key: string; value: number; label: string }> }) {
    const max = Math.max(...items.map((i) => i.value), 1);
    return (
        <div className="flex items-end gap-px h-24">
            {items.map(({ key, value, label }) => (
                <div
                    key={key}
                    className="flex-1 flex flex-col items-center justify-end h-full min-w-0"
                >
                    <div
                        className={`w-full ${value > 0 ? 'bg-accent-primary' : 'bg-bg-tertiary'}`}
                        style={{ height: `${value > 0 ? Math.max(4, (value / max) * 85) : 2}%` }}
                        title={`${value}`}
                    />
                    <div className="text-[10px] leading-3 text-fg-tertiary h-3 overflow-hidden">
                        {label}
                    </div>
                </div>
            ))}
        </div>
    );
}

/** The backend buckets by UTC hour; show the clubroom's local clock */
function localHourly(hourly: Insights['room']['hourly']): Array<{ hour: number; plays: number }> {
    const offsetHours = Math.round(-new Date().getTimezoneOffset() / 60);
    const byLocal = new Map<number, number>();
    for (const { hour, plays } of hourly) {
        const local = (((hour + offsetHours) % 24) + 24) % 24;
        byLocal.set(local, (byLocal.get(local) ?? 0) + plays);
    }
    return Array.from({ length: 24 }, (_, hour) => ({ hour, plays: byLocal.get(hour) ?? 0 }));
}

/** Fill in the days that had no plays so the chart has a continuous axis */
function lastDays(
    daily: Insights['room']['daily'],
    count: number
): Array<{ day: string; plays: number }> {
    const byDay = new Map(daily.map((d) => [d.day, d.plays]));
    const out: Array<{ day: string; plays: number }> = [];
    for (let i = count - 1; i >= 0; i--) {
        const date = new Date(Date.now() - i * 86_400_000);
        const day = date.toISOString().slice(0, 10);
        out.push({ day, plays: byDay.get(day) ?? 0 });
    }
    return out;
}
