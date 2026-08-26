/**
 * Runtime configuration loader
 * Loads config from /config.json (served by the static server with env vars)
 * Falls back to import.meta.env for build-time config, then to sensible defaults
 */

export interface AppConfig {
    /** Backend base URL for HTTP requests, e.g. https://mb.rwbk.fi */
    backendHttpUrl: string;
    /** Backend WebSocket URL derived from the base URL, e.g. wss://mb.rwbk.fi/ws */
    backendWsUrl: string;
    authEnabled: boolean;
}

let cachedConfig: AppConfig | null = null;

/**
 * Get application configuration
 * Attempts to load from /config.json for runtime config (Docker deployments)
 * Falls back to Vite env vars or window.location-based defaults
 */
export async function getConfig(): Promise<AppConfig> {
    if (cachedConfig) {
        return cachedConfig;
    }

    // Try runtime config first (from server.ts /config.json endpoint)
    try {
        const response = await fetch('/config.json');
        if (response.ok) {
            const runtimeConfig = await response.json();
            cachedConfig = {
                ...resolveBackendUrls(runtimeConfig.VITE_BACKEND_URL),
                authEnabled: runtimeConfig.VITE_AUTH_ENABLED !== 'false',
            };
            console.log('Loaded runtime config:', cachedConfig);
            return cachedConfig;
        }
    } catch {
        // Runtime config not available, fall back to build-time config
    }

    // Fall back to Vite build-time env or defaults
    cachedConfig = {
        ...resolveBackendUrls(import.meta.env.VITE_BACKEND_URL),
        authEnabled: import.meta.env.VITE_AUTH_ENABLED !== 'false',
    };

    console.log('Using build-time/default config:', cachedConfig);
    return cachedConfig;
}

/**
 * Get config synchronously (must call getConfig() first to initialize)
 */
export function getConfigSync(): AppConfig {
    if (!cachedConfig) {
        // Return defaults if not initialized
        return { ...resolveBackendUrls(undefined), authEnabled: true };
    }
    return cachedConfig;
}

/**
 * Derive both backend URLs from a single base URL (VITE_BACKEND_URL).
 * HTTP: the base as given (trailing slash stripped). WS: same origin with http(s) -> ws(s), plus /ws.
 * With no base configured, fall back to the page's host on port 3001, matching the page's scheme so an
 * HTTPS deployment never attempts a mixed-content http:// or ws:// request (browsers block those outright).
 */
export function resolveBackendUrls(base: string | undefined): Pick<AppConfig, 'backendHttpUrl' | 'backendWsUrl'> {
    const httpUrl = (base && base.trim()) || getDefaultBackendUrl();
    const backendHttpUrl = httpUrl.replace(/\/+$/, '');
    const backendWsUrl = `${backendHttpUrl.replace(/^http/i, 'ws')}/ws`;
    return { backendHttpUrl, backendWsUrl };
}

function getDefaultBackendUrl(): string {
    const scheme = window.location.protocol === 'https:' ? 'https' : 'http';
    return `${scheme}://${window.location.hostname}:3001`;
}
