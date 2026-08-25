import { useStore } from '@nanostores/preact';
import { useEffect } from 'preact/hooks';
import type { PlaybackOptions } from '../services/mopidy';
import * as mopidy from '../services/mopidy';
import { playbackOptions, setPlaybackOptions } from '../stores/player';
import { toastError } from '../stores/toast';

const FLAGS: Array<{ key: keyof PlaybackOptions; letter: string; title: string }> = [
    { key: 'repeat', letter: 'r', title: 'Repeat' },
    { key: 'random', letter: 'z', title: 'Random/Shuffle' },
    { key: 'single', letter: 's', title: 'Single' },
    { key: 'consume', letter: 'c', title: 'Consume' },
];

/**
 * ncmpcpp-style playback flags display
 * Shows [rzsc] where each letter is active/inactive
 * r = repeat, z = random, s = single, c = consume
 *
 * State lives in the shared store, which the Mopidy client keeps in sync via
 * `options_changed` events – so every phone shows what the player is really doing.
 */
export function PlaybackFlags() {
    const options = useStore(playbackOptions);

    useEffect(() => {
        if (options === null) {
            mopidy
                .getPlaybackOptions()
                .then(setPlaybackOptions)
                .catch((err) => {
                    console.error('Failed to load playback options:', err);
                });
        }
    }, [options]);

    const toggleOption = async (key: keyof PlaybackOptions) => {
        if (!options) return;
        const newValue = !options[key];

        // Optimistic update; the options_changed event confirms (or corrects) it
        setPlaybackOptions({ ...options, [key]: newValue });

        try {
            switch (key) {
                case 'repeat':
                    await mopidy.setRepeat(newValue);
                    break;
                case 'random':
                    await mopidy.setRandom(newValue);
                    break;
                case 'single':
                    await mopidy.setSingle(newValue);
                    break;
                case 'consume':
                    await mopidy.setConsume(newValue);
                    break;
            }
        } catch (err) {
            console.error(`Failed to toggle ${key}:`, err);
            setPlaybackOptions(options);
            toastError(`Could not change ${key}`);
        }
    };

    if (!options) {
        return (
            <div className="font-mono text-xs text-fg-secondary inline-flex items-center select-none">
                [____]
            </div>
        );
    }

    return (
        <div className="font-mono text-xs text-fg-secondary inline-flex items-center select-none">
            [
            {FLAGS.map(({ key, letter, title }) => (
                <button
                    type="button"
                    key={key}
                    className={`playback-flag ${options[key] ? 'playback-flag-active' : ''}`}
                    onClick={() => toggleOption(key)}
                    title={title}
                    aria-label={title}
                    aria-pressed={options[key]}
                >
                    {options[key] ? letter : '_'}
                </button>
            ))}
            ]
        </div>
    );
}
