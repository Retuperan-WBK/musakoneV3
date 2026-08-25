import type { JSX } from 'preact';
import { useRef, useState } from 'preact/hooks';

interface SwipeableItemProps {
    children: JSX.Element;
    isDisabled?: boolean;
    onSwipeLeft?: () => void;
    onSwipeRight?: () => void;
    leftLabel?: string;
    rightLabel?: string;
    threshold?: number;
    className?: string;
    wrapperClassName?: string;
}

/** Movement (px) before we decide whether the gesture is a horizontal swipe or a vertical scroll */
const AXIS_LOCK_DISTANCE = 10;
/** Extra travel allowed past the threshold so the row visibly "commits" */
const OVERSHOOT = 40;

/**
 * A swipeable item component that triggers actions on left/right swipe
 * Used for track items in Library and Search views
 */
export function SwipeableItem({
    children,
    isDisabled = false,
    onSwipeLeft,
    onSwipeRight,
    leftLabel = '+ Add Next',
    rightLabel = '+ Add to End',
    threshold = 80,
    className = '',
    wrapperClassName = '',
}: SwipeableItemProps) {
    const [swipeX, setSwipeX] = useState(0);
    const [animating, setAnimating] = useState<'left' | 'right' | null>(null);
    const startX = useRef(0);
    const startY = useRef(0);
    /** null = undecided, 'x' = horizontal swipe, 'y' = the browser is scrolling */
    const axis = useRef<'x' | 'y' | null>(null);
    const swiping = useRef(false);
    const swipeXRef = useRef(0);
    const maxTravel = threshold + OVERSHOOT;

    const updateSwipeX = (x: number) => {
        swipeXRef.current = x;
        setSwipeX(x);
    };

    const handleTouchStart = (e: TouchEvent) => {
        if (isDisabled || animating) return;
        const touch = e.touches[0];
        if (!touch) return;
        startX.current = touch.clientX;
        startY.current = touch.clientY;
        axis.current = null;
        swiping.current = true;
    };

    const handleTouchMove = (e: TouchEvent) => {
        if (!swiping.current || isDisabled || animating) return;
        const touch = e.touches[0];
        if (!touch) return;
        const dx = touch.clientX - startX.current;
        const dy = touch.clientY - startY.current;

        if (axis.current === null) {
            if (Math.abs(dx) < AXIS_LOCK_DISTANCE && Math.abs(dy) < AXIS_LOCK_DISTANCE) return;
            // Mostly vertical: this is a scroll, leave the row alone for the rest of the gesture
            axis.current = Math.abs(dy) > Math.abs(dx) ? 'y' : 'x';
        }
        if (axis.current === 'y') return;

        updateSwipeX(Math.max(-maxTravel, Math.min(maxTravel, dx)));
    };

    const finishSwipe = () => {
        if (!swiping.current) return;
        swiping.current = false;
        if (isDisabled || animating || axis.current !== 'x') {
            updateSwipeX(0);
            return;
        }

        const x = swipeXRef.current;
        const direction =
            x < -threshold && onSwipeLeft ? 'left' : x > threshold && onSwipeRight ? 'right' : null;
        if (!direction) {
            updateSwipeX(0);
            return;
        }

        setAnimating(direction);
        setTimeout(() => {
            (direction === 'left' ? onSwipeLeft : onSwipeRight)?.();
            setTimeout(() => {
                updateSwipeX(0);
                setAnimating(null);
            }, 150);
        }, 200);
    };

    const handleTouchCancel = () => {
        // The browser took over the gesture (scroll, system gesture): reset without triggering
        swiping.current = false;
        axis.current = null;
        updateSwipeX(0);
    };

    const getSwipeIndicator = () => {
        if (animating === 'left') return 'swipe-left-active';
        if (animating === 'right') return 'swipe-right-active';
        if (swipeX < -threshold) return 'swipe-left-active';
        if (swipeX > threshold) return 'swipe-right-active';
        if (swipeX < -20) return 'swipe-left';
        if (swipeX > 20) return 'swipe-right';
        return '';
    };

    const getTransform = () => {
        if (animating === 'left') return 'translateX(-100%)';
        if (animating === 'right') return 'translateX(100%)';
        return `translateX(${swipeX}px)`;
    };

    return (
        <div
            className={`relative overflow-hidden w-full ${getSwipeIndicator()} ${wrapperClassName}`}
        >
            {onSwipeLeft && <div className="swipe-hint swipe-hint-left">{leftLabel}</div>}
            {onSwipeRight && <div className="swipe-hint swipe-hint-right">{rightLabel}</div>}
            <div
                className={`relative z-1 bg-bg-primary transition-transform duration-150 ${animating ? 'duration-200' : ''} ${className}`}
                style={{ transform: getTransform(), touchAction: 'pan-y' }}
                onTouchStart={handleTouchStart}
                onTouchMove={handleTouchMove}
                onTouchEnd={finishSwipe}
                onTouchCancel={handleTouchCancel}
            >
                {children}
            </div>
        </div>
    );
}
