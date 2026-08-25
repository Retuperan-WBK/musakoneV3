import { Library, List, ListMusic, Search, Sparkles } from 'lucide-preact';
import { useLocation } from 'wouter';

export function BottomNav() {
    const [location, setLocation] = useLocation();

    const navItems = [
        { path: '/', icon: List, label: 'Queue' },
        { path: '/library', icon: Library, label: 'Library' },
        { path: '/search', icon: Search, label: 'Search' },
        { path: '/playlists', icon: ListMusic, label: 'Playlists' },
        { path: '/mix', icon: Sparkles, label: 'Mix' },
    ];

    return (
        <nav
            className="relative w-full h-[var(--bottom-nav-height)] bg-bg-secondary border-t border-border-primary flex justify-around items-center bottom-nav-after"
            style={{ flexShrink: 0 }}
        >
            {navItems.map((item) => {
                const Icon = item.icon;
                const isActive =
                    item.path === '/' ? location === '/' : location.startsWith(item.path);

                return (
                    <button
                        type="button"
                        key={item.path}
                        onClick={() => setLocation(item.path)}
                        className={`flex-1 flex flex-col items-center justify-center gap-1 min-h-12 text-sm cursor-pointer bg-transparent border-none p-0 transition-colors duration-150 active:scale-95 ${isActive ? 'text-accent-primary' : 'text-fg-secondary'}`}
                        aria-label={item.label}
                        aria-current={isActive ? 'page' : undefined}
                    >
                        <Icon className="w-6 h-6" />
                        <span>{item.label}</span>
                    </button>
                );
            })}
        </nav>
    );
}
