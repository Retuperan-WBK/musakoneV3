import type { JSX } from 'preact';
import { useLayoutEffect, useMemo, useRef, useState } from 'preact/hooks';
import type { LibraryRef } from '../services/mopidy';

interface LibraryListProps {
    items: LibraryRef[];
    initialScroll: number;
    onScroll: (top: number) => void;
    renderItem: (item: LibraryRef) => JSX.Element;
}

// Library rows have two layouts: tracks and folders. Measure their actual heights
// so browser zoom/font changes work without mounting thousands of swipe handlers.
export function LibraryList({ items, initialScroll, onScroll, renderItem }: LibraryListProps) {
    const container = useRef<HTMLDivElement>(null);
    const raf = useRef<number>();
    const [top, setTop] = useState(initialScroll);
    const [height, setHeight] = useState(600);
    const [sizes, setSizes] = useState([48, 56]);
    const [focused, setFocused] = useState(-1);
    const windowed = items.length > 500;
    const offsets = useMemo(() => {
        const result = [0];
        for (const item of items) {
            result.push((result.at(-1) ?? 0) + (sizes[item.type === 'track' ? 0 : 1] ?? 56));
        }
        return result;
    }, [items, sizes]);
    const rowAt = (position: number) => {
        let lo = 0;
        let hi = items.length;
        while (lo < hi) {
            const mid = (lo + hi) >>> 1;
            if ((offsets[mid + 1] ?? 0) <= position) lo = mid + 1;
            else hi = mid;
        }
        return Math.min(lo, Math.max(0, items.length - 1));
    };
    const start = windowed ? Math.max(0, rowAt(top) - 8) : 0;
    const end = windowed ? Math.min(items.length, rowAt(top + height) + 9) : items.length;
    const indices = Array.from({ length: end - start }, (_, i) => start + i);
    // Keep a focused control alive when touch/wheel scrolling moves it offscreen.
    if (focused >= 0 && focused < items.length && (focused < start || focused >= end)) {
        indices.push(focused);
        indices.sort((a, b) => a - b);
    }

    useLayoutEffect(() => {
        const element = container.current;
        if (!element) return;
        element.scrollTop = initialScroll;
        setHeight(element.clientHeight);
        return () => {
            if (raf.current !== undefined) cancelAnimationFrame(raf.current);
        };
    }, [initialScroll]);

    // biome-ignore lint/correctness/useExhaustiveDependencies: new window rows must be observed after scrolling
    useLayoutEffect(() => {
        const element = container.current;
        if (!element) return;
        const measure = () => {
            setHeight(element.clientHeight);
            const next = [...sizes];
            for (const row of element.querySelectorAll<HTMLElement>('[data-library-kind]')) {
                const kind = Number(row.dataset.libraryKind);
                const measured = row.getBoundingClientRect().height;
                if (measured > 0) next[kind] = measured;
            }
            if (next.some((size, i) => size !== sizes[i])) setSizes(next);
        };
        measure();
        const observer = new ResizeObserver(measure);
        observer.observe(element);
        for (const row of element.querySelectorAll('[data-library-kind]')) observer.observe(row);
        return () => observer.disconnect();
    }, [start, end, sizes]);

    return (
        <div
            ref={container}
            className="flex-1 overflow-y-auto overscroll-y-contain pb-2"
            onScroll={(event) => {
                const value = event.currentTarget.scrollTop;
                onScroll(value);
                if (raf.current !== undefined) cancelAnimationFrame(raf.current);
                raf.current = requestAnimationFrame(() => setTop(value));
            }}
        >
            <ul
                className="list-none"
                aria-label="Library items"
                style={windowed ? { position: 'relative', height: offsets.at(-1) } : undefined}
            >
                {indices.map((index) => {
                    const item = items[index];
                    if (!item) return null;
                    return (
                        <li
                            key={item.uri}
                            aria-posinset={index + 1}
                            aria-setsize={items.length}
                            data-library-kind={item.type === 'track' ? 0 : 1}
                            style={
                                windowed
                                    ? { position: 'absolute', top: offsets[index], width: '100%' }
                                    : undefined
                            }
                            onFocusCapture={() => setFocused(index)}
                        >
                            {renderItem(item)}
                        </li>
                    );
                })}
            </ul>
        </div>
    );
}
