import { useStore } from '@nanostores/preact';
import type { ComponentChildren } from 'preact';
import { useEffect } from 'preact/hooks';
import { useLocation } from 'wouter';
import { isAuthenticated } from '../services/auth';
import { currentUser } from '../stores/auth';

interface ProtectedRouteProps {
    children: ComponentChildren;
}

/**
 * Wrapper component that redirects to /login if user is not authenticated.
 * Reacts to the auth store, so clearing the session (logout, expired token) redirects immediately.
 */
export function ProtectedRoute({ children }: ProtectedRouteProps) {
    const user = useStore(currentUser);
    const [, setLocation] = useLocation();
    const hasAuth = user !== null || isAuthenticated();

    useEffect(() => {
        if (!hasAuth) {
            setLocation('/login');
        }
    }, [hasAuth, setLocation]);

    if (!hasAuth) {
        return null;
    }

    return <>{children}</>;
}
