import { atom } from 'nanostores';
import type { LibraryRef } from './mopidy';

const TTL_MS = 5 * 60 * 1000;
const MAX_FOLDERS = 64;
const MAX_BYTES = 8 * 1024 * 1024;
const folders = new Map<string, { items: LibraryRef[]; expires: number; bytes: number }>();
const pending = new Map<string, Promise<LibraryRef[]>>();
let bytes = 0;
let generation = 0;

export const libraryRevision = atom(0);

export function cachedBrowse(uri: string | null): LibraryRef[] | undefined {
    const key = uri ?? '';
    const entry = folders.get(key);
    if (!entry) return undefined;
    folders.delete(key);
    if (entry.expires <= Date.now()) {
        bytes -= entry.bytes;
        return undefined;
    }
    folders.set(key, entry);
    return entry.items;
}

export function loadBrowse(
    uri: string | null,
    loader: () => Promise<LibraryRef[]>
): Promise<LibraryRef[]> {
    const cached = cachedBrowse(uri);
    if (cached) return Promise.resolve(cached);
    const key = uri ?? '';
    const existing = pending.get(key);
    if (existing) return existing;
    const version = generation;
    const request = loader()
        .then((items) => {
            // Approximate retained JS strings/objects, bounded as well as folder count.
            const size = items.reduce(
                (sum, item) =>
                    sum + 128 + 2 * (item.uri.length + item.name.length + item.type.length),
                key.length * 2
            );
            if (version === generation && size <= MAX_BYTES) {
                while (folders.size >= MAX_FOLDERS || bytes + size > MAX_BYTES) {
                    const oldest = folders.entries().next().value;
                    if (!oldest) break;
                    bytes -= oldest[1].bytes;
                    folders.delete(oldest[0]);
                }
                folders.set(key, { items, expires: Date.now() + TTL_MS, bytes: size });
                bytes += size;
            }
            return items;
        })
        .finally(() => {
            if (pending.get(key) === request) pending.delete(key);
        });
    pending.set(key, request);
    return request;
}

export function invalidateBrowseCache(notify = true): void {
    generation++;
    folders.clear();
    pending.clear();
    bytes = 0;
    if (notify) libraryRevision.set(libraryRevision.get() + 1);
}
