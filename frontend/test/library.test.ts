import { beforeEach, expect, mock, test } from 'bun:test';
import { cachedBrowse, invalidateBrowseCache, loadBrowse } from '../src/services/library-cache';
import type { LibraryRef } from '../src/services/mopidy';

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (error: Error) => void;
    const promise = new Promise<T>((yes, no) => {
        resolve = yes;
        reject = no;
    });
    return { promise, resolve, reject };
}

const requests = new Map<string, ReturnType<typeof deferred<LibraryRef[]>>>();
const browse = mock((uri?: string) =>
    loadBrowse(uri ?? null, () => {
        const request = deferred<LibraryRef[]>();
        requests.set(uri ?? '', request);
        return request.promise;
    })
);
mock.module('../src/services/mopidy', () => ({ browse }));
const library = await import('../src/stores/library');
const refs = (name: string): LibraryRef[] => [{ type: 'directory', uri: name, name }];

beforeEach(() => {
    invalidateBrowseCache();
    library.resetLibrary();
    requests.clear();
    browse.mockClear();
});

test('browse shares pending requests, caches empty folders, and retries errors', async () => {
    const result = deferred<LibraryRef[]>();
    const loader = mock(() => result.promise);
    const a = loadBrowse('a', loader);
    expect(loadBrowse('a', loader)).toBe(a);
    expect(loader).toHaveBeenCalledTimes(1);
    result.resolve([]);
    await a;
    expect(cachedBrowse('a')).toEqual([]);
    await expect(loadBrowse('bad', () => Promise.reject(new Error('offline')))).rejects.toThrow();
    await expect(loadBrowse('bad', async () => refs('ok'))).resolves.toEqual(refs('ok'));
});

test('invalidation prevents old responses from replacing newer cached data', async () => {
    const old = deferred<LibraryRef[]>();
    const first = loadBrowse('a', () => old.promise);
    invalidateBrowseCache();
    await loadBrowse('a', async () => refs('new'));
    old.resolve(refs('old'));
    await first;
    expect(cachedBrowse('a')).toEqual(refs('new'));
});

test('cache has LRU, size and expiry bounds', async () => {
    await loadBrowse('first', async () => refs('first'));
    for (let i = 0; i < 63; i++) await loadBrowse(String(i), async () => refs(String(i)));
    cachedBrowse('first');
    await loadBrowse('last', async () => refs('last'));
    expect(cachedBrowse('0')).toBeUndefined();
    expect(cachedBrowse('first')).toEqual(refs('first'));
    const huge = refs('x'.repeat(5 * 1024 * 1024));
    expect(await loadBrowse('huge', async () => huge)).toBe(huge);
    expect(cachedBrowse('huge')).toBeUndefined();
    const now = Date.now;
    Date.now = () => now() + 6 * 60 * 1000;
    try {
        expect(cachedBrowse('first')).toBeUndefined();
    } finally {
        Date.now = now;
    }
});

test('Back restores cached items and scroll while another folder is loading', async () => {
    await loadBrowse(null, async () => refs('A'));
    await library.loadLibrary();
    library.saveLibraryScroll(420);
    library.navigateTo('A', 'A');
    const pending = library.loadLibrary();
    expect(library.libraryLoading.get()).toBe(true);
    expect(library.libraryItems.get()).toEqual([]);
    library.navigateBack();
    expect(library.libraryItems.get()).toEqual(refs('A'));
    expect(library.libraryLoading.get()).toBe(false);
    expect(library.libraryPath.get()[0]?.scrollTop).toBe(420);
    requests.get('A')?.reject(new Error('late failure'));
    await pending;
    expect(library.libraryError.get()).toBe(null);
    expect(library.libraryItems.get()).toEqual(refs('A'));
});

test('A to B to A shares the request and only the newest navigation completes', async () => {
    library.navigateTo('A', 'A');
    const oldA = library.loadLibrary();
    library.navigateTo('B', 'B');
    const b = library.loadLibrary();
    library.navigateBack();
    const newA = library.loadLibrary();
    requests.get('B')?.resolve(refs('wrong'));
    await b;
    expect(library.libraryLoading.get()).toBe(true);
    requests.get('A')?.resolve(refs('right'));
    await Promise.all([oldA, newA]);
    expect(library.libraryItems.get()).toEqual(refs('right'));
    expect(library.libraryLoading.get()).toBe(false);
});
