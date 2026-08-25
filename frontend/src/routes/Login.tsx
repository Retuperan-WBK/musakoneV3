import { useStore } from '@nanostores/preact';
import { useState } from 'preact/hooks';
import { useLocation } from 'wouter';
import { login } from '../services/auth';
import { authError, authLoading, setAuthError, setAuthLoading, setUser } from '../stores/auth';

export function Login() {
    const [username, setUsername] = useState('');
    const [password, setPassword] = useState('');
    const loading = useStore(authLoading);
    const error = useStore(authError);
    const [, setLocation] = useLocation();

    const handleSubmit = async (e: Event) => {
        e.preventDefault();

        if (!username.trim() || !password.trim()) {
            setAuthError('Username and password are required');
            return;
        }

        setAuthLoading(true);
        setAuthError(null);

        try {
            const response = await login(username, password);
            setUser(response.user);
            setLocation('/');
        } catch (err) {
            setAuthError(err instanceof Error ? err.message : 'Login failed');
        } finally {
            setAuthLoading(false);
        }
    };

    return (
        <div className="flex flex-col items-center h-full overflow-y-auto p-4 bg-bg-primary">
            <div className="w-full max-w-[400px] my-auto p-6 md:p-8 bg-bg-secondary border border-border-primary shadow-lg">
                <h1 className="m-0 mb-2 text-xl font-semibold text-accent-primary text-center">
                    MusakoneV3
                </h1>
                <p className="m-0 mb-8 text-sm text-fg-secondary text-center">
                    Sign in to continue
                </p>

                {error && (
                    <div className="p-4 text-sm text-error bg-error/10 border border-error rounded-sm text-center mb-4">
                        {error}
                    </div>
                )}

                <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
                    <div className="flex flex-col gap-2">
                        <label className="text-sm font-medium text-fg-primary" htmlFor="username">
                            Username
                        </label>
                        <input
                            id="username"
                            type="text"
                            className="h-14 md:h-12 px-4 font-mono text-base text-fg-primary bg-bg-primary border border-border-primary outline-none transition-colors duration-200 focus:border-accent-primary placeholder:text-fg-tertiary"
                            value={username}
                            onInput={(e) => setUsername((e.target as HTMLInputElement).value)}
                            placeholder="Enter username"
                            disabled={loading}
                            autoComplete="username"
                        />
                    </div>

                    <div className="flex flex-col gap-2">
                        <label className="text-sm font-medium text-fg-primary" htmlFor="password">
                            Password
                        </label>
                        <input
                            id="password"
                            type="password"
                            className="h-14 md:h-12 px-4 font-mono text-base text-fg-primary bg-bg-primary border border-border-primary outline-none transition-colors duration-200 focus:border-accent-primary placeholder:text-fg-tertiary"
                            value={password}
                            onInput={(e) => setPassword((e.target as HTMLInputElement).value)}
                            placeholder="Enter password"
                            disabled={loading}
                            autoComplete="current-password"
                        />
                    </div>

                    <button
                        type="submit"
                        className="h-14 md:h-12 mt-2 font-mono text-base font-semibold text-bg-primary bg-accent-primary border-none cursor-pointer transition-opacity duration-200 uppercase tracking-wide hover:opacity-90 active:opacity-80 disabled:opacity-50 disabled:cursor-not-allowed"
                        disabled={loading}
                    >
                        {loading ? 'Signing in...' : 'Sign In'}
                    </button>
                </form>

                <p className="mt-6 text-sm text-fg-secondary text-center">
                    Don't have an account?{' '}
                    <a
                        href="/register"
                        className="text-accent-primary border-b border-transparent hover:border-accent-primary transition-colors duration-200"
                    >
                        Create one
                    </a>
                </p>
            </div>
        </div>
    );
}
