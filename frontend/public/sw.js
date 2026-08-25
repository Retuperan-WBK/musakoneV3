// Bump the version whenever the caching strategy changes; old caches are dropped on activate.
const CACHE_NAME = 'musakone-v3';
const APP_SHELL = ['/', '/manifest.json', '/favicon.svg', '/icon-192.png', '/icon-512.png'];

self.addEventListener('install', (event) => {
    event.waitUntil(caches.open(CACHE_NAME).then((cache) => cache.addAll(APP_SHELL)));
    self.skipWaiting();
});

self.addEventListener('activate', (event) => {
    event.waitUntil(
        caches
            .keys()
            .then((names) =>
                Promise.all(
                    names.filter((name) => name !== CACHE_NAME).map((name) => caches.delete(name))
                )
            )
    );
    self.clients.claim();
});

async function cachePut(request, response) {
    const cache = await caches.open(CACHE_NAME);
    await cache.put(request, response);
}

/** Network first, falling back to the cached copy */
async function networkFirst(request, fallbackKey) {
    try {
        const response = await fetch(request);
        if (response.ok) {
            cachePut(fallbackKey || request, response.clone());
        }
        return response;
    } catch (err) {
        const cached = await caches.match(fallbackKey || request);
        if (cached) return cached;
        throw err;
    }
}

/** Cache first for content-hashed build assets, which never change under the same URL */
async function cacheFirst(request) {
    const cached = await caches.match(request);
    if (cached) return cached;
    const response = await fetch(request);
    if (response.ok) {
        cachePut(request, response.clone());
    }
    return response;
}

self.addEventListener('fetch', (event) => {
    const { request } = event;
    if (request.method !== 'GET') return;

    const url = new URL(request.url);
    // Backend API / WebSocket live on another origin; runtime config must never be stale
    if (url.origin !== self.location.origin) return;
    if (url.pathname === '/config.json' || url.pathname.startsWith('/api')) return;

    // Any in-app route (/search, /playlists/3, …) is the same SPA shell
    if (request.mode === 'navigate') {
        event.respondWith(networkFirst(request, '/'));
        return;
    }

    if (url.pathname.startsWith('/assets/') || url.pathname.startsWith('/fonts/')) {
        event.respondWith(cacheFirst(request));
        return;
    }

    event.respondWith(networkFirst(request));
});
