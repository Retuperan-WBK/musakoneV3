import { defineConfig, presetWind4 } from 'unocss';

export default defineConfig({
    presets: [presetWind4()],
    theme: {
        colors: {
            bg: {
                primary: '#000000',
                secondary: '#0a0a0a',
                tertiary: '#141414',
            },
            fg: {
                primary: '#ffffff',
                secondary: '#b0b0b0',
                tertiary: '#707070',
            },
            accent: {
                primary: '#cc0000',
                secondary: '#ff3333',
                dim: '#880000',
            },
            border: {
                primary: '#333333',
                secondary: '#1a1a1a',
            },
            error: '#ff4444',
            warning: '#ffaa00',
            success: '#00cc44',
        },
        font: {
            mono: '"VT323", monospace',
            sans: '"VT323", monospace',
        },
    },
    shortcuts: {
        // Touch targets
        'touch-target': 'min-h-12 min-w-12',
        'touch-target-sm': 'min-h-8 min-w-8',

        // Common button styles
        btn: 'flex items-center justify-center cursor-pointer transition-all duration-150',
        'btn-icon':
            'btn w-8 h-8 bg-transparent border border-border-primary text-fg-tertiary hover:text-accent-primary hover:border-accent-primary active:bg-bg-secondary shrink-0',
        'btn-control':
            'btn w-12 h-12 bg-bg-secondary border border-border-primary text-fg-primary rounded hover:border-accent-primary active:scale-95 active:bg-bg-primary disabled:opacity-30 disabled:cursor-not-allowed',

        // Track item
        'track-item':
            'flex items-center gap-2 px-4 py-1 bg-bg-primary border-b-2 border-border-secondary min-h-10 w-full transition-colors duration-150',

        // In queue indicator
        'in-queue': 'flex items-center justify-center w-8 h-8 text-success shrink-0',

        // Status dots
        'status-dot': 'w-2 h-2 rounded-full bg-fg-tertiary',
        'status-dot-connected': 'bg-success',
        'status-dot-connecting': 'bg-warning animate-pulse',
        'status-dot-error': 'bg-error',

        // Playback flag
        'playback-flag':
            'bg-transparent border-none p-0 m-0 font-mono text-xs text-fg-tertiary cursor-pointer min-w-[0.6em] text-center transition-colors duration-100 hover:text-fg-primary',
        'playback-flag-active': 'text-accent-primary',

        // Swipe hints
        'swipe-hint':
            'absolute top-0 bottom-0 flex items-center px-4 text-sm uppercase opacity-0 transition-opacity duration-150',
        'swipe-hint-left': 'right-0 bg-accent-primary text-bg-primary',
        'swipe-hint-right': 'left-0 bg-accent-secondary text-bg-primary',
    },
    safelist: ['status-dot-connected', 'status-dot-connecting', 'status-dot-error'],
    preflights: [
        {
            getCSS: ({ theme }) => `
                *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
                html { font-size: 16px; background: #000; height: 100%; color-scheme: dark; }
                body { font-family: ${theme.font?.mono}; line-height: 1rem; color: #fff; background: #000; -webkit-font-smoothing: antialiased; height: 100%; }
                #app { height: 100%; font-family: ${theme.font?.mono}; }
                button { font-family: ${theme.font?.mono}; cursor: pointer; outline: none; -webkit-tap-highlight-color: transparent; }
                button:focus { outline: none; }
                button:focus-visible { outline: 2px solid #cc0000; outline-offset: 2px; }
                input, textarea, select { font-family: ${theme.font?.mono}; }
                a { color: #cc0000; text-decoration: none; }
                a:hover { text-decoration: underline; }
                * { scrollbar-width: none; }
                *::-webkit-scrollbar { display: none; }
            `,
        },
    ],
});
