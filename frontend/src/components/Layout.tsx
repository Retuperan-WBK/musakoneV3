import { useStore } from '@nanostores/preact';
import type { ComponentChildren } from 'preact';
import { useLocation } from 'wouter';
import { logout } from '../services/auth';
import * as mopidy from '../services/mopidy';
import { currentUser, setUser } from '../stores/auth';
import { connectionStatus } from '../stores/connection';
import { AddToPlaylistModal } from './AddToPlaylistModal';
import { BottomNav } from './BottomNav';
import { ConfirmModal } from './ConfirmModal';
import { MiniPlayer } from './MiniPlayer';
import { Toast } from './Toast';

interface LayoutProps {
    children: ComponentChildren;
}

export function Layout({ children }: LayoutProps) {
    const status = useStore(connectionStatus);
    const user = useStore(currentUser);
    const [location, setLocation] = useLocation();

    const getStatusLabel = () => {
        switch (status) {
            case 'connected':
                return 'Connected';
            case 'connecting':
                return 'Connecting...';
            case 'error':
                return 'Error';
            default:
                return 'Disconnected';
        }
    };

    const getStatusDotClass = () => {
        switch (status) {
            case 'connected':
                return 'status-dot-connected';
            case 'connecting':
                return 'status-dot-connecting';
            case 'error':
                return 'status-dot-error';
            default:
                return '';
        }
    };

    const handleLogout = () => {
        logout();
        setUser(null);
        mopidy.disconnect();
        setLocation('/login');
    };

    return (
        <div className="app-root">
            {/* Row 1: Header */}
            <header className="bg-bg-secondary px-4 py-2 border-b border-border-primary flex justify-between items-center z-10">
                <h1 className="m-0 text-accent-primary text-base flex-1 text-left">MusakoneV3</h1>
                <div className="flex items-center gap-2 text-sm">
                    <div className={`status-dot ${getStatusDotClass()}`} />
                    <span>{getStatusLabel()}</span>
                    {user && (
                        <button
                            type="button"
                            className="ml-3 px-3 py-1 font-mono text-xs text-fg-secondary bg-transparent border border-border-primary cursor-pointer transition-all duration-200 hover:text-accent-primary hover:border-accent-primary active:opacity-70"
                            onClick={handleLogout}
                        >
                            Logout
                        </button>
                    )}
                </div>
            </header>

            {/* Row 2: Scrollable content */}
            <main className="app-main flex flex-col">{children}</main>

            {/* Row 3: Bottom controls – BottomNav is in-flow; MiniPlayer is fixed above it */}
            <div className="bottom-controls">
                <BottomNav />
            </div>

            {location === '/' && (
                <div
                    style={{
                        position: 'fixed',
                        left: 0,
                        right: 0,
                        bottom: 'var(--bottom-nav-height)',
                        zIndex: 120,
                    }}
                >
                    <MiniPlayer />
                </div>
            )}

            <AddToPlaylistModal />
            <ConfirmModal />
            <Toast />
        </div>
    );
}
