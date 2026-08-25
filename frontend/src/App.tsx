import { useStore } from '@nanostores/preact';
import { useEffect } from 'preact/hooks';
import { Redirect, Route, Switch, useLocation } from 'wouter';
import { Layout } from './components/Layout';
import { ProtectedRoute } from './components/ProtectedRoute';
import { LibraryView } from './routes/LibraryView';
import { Login } from './routes/Login';
import { MixView } from './routes/MixView';
import { PlaylistDetailView } from './routes/PlaylistDetailView';
import { PlaylistsView } from './routes/PlaylistsView';
import { QueueView } from './routes/QueueView';
import { Register } from './routes/Register';
import { SearchView } from './routes/SearchView';
import { getCurrentUser, isAuthenticated, logout } from './services/auth';
import * as mopidy from './services/mopidy';
import { clearAuth, currentUser, setAuthLoading, setUser } from './stores/auth';

export const App = () => {
    const user = useStore(currentUser);
    const [, setLocation] = useLocation();

    // Load user on app startup if token exists
    useEffect(() => {
        const loadUser = async () => {
            if (isAuthenticated()) {
                setAuthLoading(true);
                try {
                    const userData = await getCurrentUser();
                    setUser(userData);
                } catch (err) {
                    // Expired/invalid token: drop it and go to the login form instead of a blank page
                    console.error('Failed to load user:', err);
                    logout();
                    clearAuth();
                    setLocation('/login');
                } finally {
                    setAuthLoading(false);
                }
            }
        };

        loadUser();
    }, []);

    useEffect(() => {
        // Connect to backend WebSocket on mount (only if authenticated)
        if (user) {
            mopidy.connect().catch(console.error);
        }

        return () => {
            mopidy.disconnect();
        };
    }, [user]);

    return (
        <div className="h-full flex flex-col overflow-hidden bg-bg-secondary text-fg-primary font-mono">
            <Switch>
                {/* Public routes */}
                <Route path="/login">
                    <Login />
                </Route>

                <Route path="/register">
                    <Register />
                </Route>

                {/* Protected routes with shared layout */}
                <Route path="/">
                    <ProtectedRoute>
                        <Layout>
                            <QueueView />
                        </Layout>
                    </ProtectedRoute>
                </Route>

                <Route path="/library">
                    <ProtectedRoute>
                        <Layout>
                            <LibraryView />
                        </Layout>
                    </ProtectedRoute>
                </Route>

                <Route path="/search">
                    <ProtectedRoute>
                        <Layout>
                            <SearchView />
                        </Layout>
                    </ProtectedRoute>
                </Route>

                <Route path="/playlists">
                    <ProtectedRoute>
                        <Layout>
                            <PlaylistsView />
                        </Layout>
                    </ProtectedRoute>
                </Route>

                <Route path="/playlists/:id">
                    <ProtectedRoute>
                        <Layout>
                            <PlaylistDetailView />
                        </Layout>
                    </ProtectedRoute>
                </Route>

                <Route path="/mix">
                    <ProtectedRoute>
                        <Layout>
                            <MixView />
                        </Layout>
                    </ProtectedRoute>
                </Route>

                <Route path="/analytics">
                    <Redirect to="/mix" />
                </Route>
            </Switch>
        </div>
    );
};
