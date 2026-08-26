/**
 * Bun static server for MusakoneV3 frontend.
 * Serves files from /dist, falls back to index.html for SPA routing.
 * Provides /config.json for runtime configuration.
 */

// Runtime config from environment variables
// VITE_BACKEND_URL is the backend base URL (e.g. https://mb.rwbk.fi); the client derives the
// WebSocket URL from it. VITE_BACKEND_HTTP_URL is accepted as a legacy alias for existing deployments.
const runtimeConfig = {
    VITE_BACKEND_URL: process.env.VITE_BACKEND_URL || process.env.VITE_BACKEND_HTTP_URL || '',
    VITE_AUTH_ENABLED: process.env.VITE_AUTH_ENABLED || 'true',
};

const serveStatic = async (req: Request): Promise<Response> => {
    const url = new URL(req.url);

    // Serve runtime config
    if (url.pathname === '/config.json') {
        return new Response(JSON.stringify(runtimeConfig), {
            headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-cache' },
        });
    }

    const path = url.pathname === '/' ? '/index.html' : url.pathname;
    const file = Bun.file(`./dist${path}`);

    if (!(await file.exists())) {
        const indexFile = Bun.file('./dist/index.html');
        return new Response(indexFile, {
            headers: { 'Content-Type': 'text/html', 'Cache-Control': 'no-cache' },
        });
    }

    // Vite emits content-hashed files under /assets – safe to cache forever. Everything else
    // (index.html, sw.js, manifest, icons) must be revalidated so deploys take effect.
    const immutable = path.startsWith('/assets/');
    return new Response(file, {
        headers: {
            'Cache-Control': immutable ? 'public, max-age=31536000, immutable' : 'no-cache',
        },
    });
};

Bun.serve({
    port: 3000,
    fetch: serveStatic,
});

console.log('MusakoneV3 Frontend running on http://localhost:3000');
