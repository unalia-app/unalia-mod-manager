"use client";
// beui.dev/components/motion/animated-toast-stack, adapted to the
// @modalia/ui tokens: each toast holds its timer (paused on hover). One with
// an action shows the time left, and a click there keeps it open.

import { AnimatePresence, motion, type Transition, useReducedMotion } from "motion/react";
import { CircleAlert, CircleCheck, Info, type LucideIcon, TriangleAlert, X } from "lucide-react";
import { memo, type ReactNode, useEffect, useRef, useState } from "react";

import { cn } from "./cn";

export type ToastTone = "neutral" | "info" | "success" | "warning" | "error";

export type ToastStackItem = {
	id: number;
	title: ReactNode;
	tone: ToastTone;
	/** Under the title. Can be technical: a long one scrolls. */
	details: string;
	action?: { label: string; onClick: () => void };
	/** En millisecondes. */
	duration: number;
};

const EASE_OUT = [0.16, 1, 0.3, 1] as const;

const STACK_SPRING: Transition = { type: "spring", stiffness: 420, damping: 34, mass: 0.75 };

const TONE_ICON: Record<ToastTone, LucideIcon> = {
	neutral: Info,
	info: Info,
	success: CircleCheck,
	warning: TriangleAlert,
	error: CircleAlert,
};

const TONE_CLASS: Record<ToastTone, string> = {
	neutral: "text-fg-muted",
	info: "text-info",
	success: "text-success",
	warning: "text-warning",
	error: "text-danger",
};

/** The line of light on the top edge. */
const EDGE_CLASS: Record<ToastTone, string> = {
	neutral: "via-white/25",
	info: "via-info/60",
	success: "via-success/60",
	warning: "via-warning/50",
	error: "via-danger/60",
};

/** What that line casts on the toast. */
const GLOW_CLASS: Record<ToastTone, string> = {
	neutral: "from-white/[0.03]",
	info: "from-info/[0.06]",
	success: "from-success/[0.06]",
	warning: "from-warning/[0.05]",
	error: "from-danger/[0.06]",
};

const TIMER_CLASS: Record<ToastTone, string> = {
	neutral: "bg-accent",
	info: "bg-info",
	success: "bg-success",
	warning: "bg-warning",
	error: "bg-danger",
};

/** After a pause, the toast stays at least this long. */
const MIN_REMAINING_MS = 800;

/**
 * Toast stack at the bottom center (the bottom right corner belongs to the
 * beta bubble). The most recent at the bottom; a swipe to the right or the
 * cross closes it.
 */
export function ToastStack({ toasts, onDismiss }: { toasts: ToastStackItem[]; onDismiss: (id: number) => void }) {
	return (
		<ol
			aria-live="polite"
			aria-atomic="false"
			className="pointer-events-none fixed bottom-6 left-1/2 z-[90] flex w-[calc(100vw-3rem)] max-w-md -translate-x-1/2 flex-col-reverse gap-2"
		>
			<AnimatePresence initial={false} mode="popLayout">
				{[...toasts].reverse().map((toast, index) => (
					<ToastItem key={toast.id} toast={toast} index={index} onDismiss={onDismiss} />
				))}
			</AnimatePresence>
		</ol>
	);
}

const ToastItem = memo(function ToastItem({
	toast,
	index,
	onDismiss,
}: {
	toast: ToastStackItem;
	index: number;
	onDismiss: (id: number) => void;
}) {
	const reduce = useReducedMotion();
	const [hovered, setHovered] = useState(false);
	const [stopped, setStopped] = useState(false);
	const [seconds, setSeconds] = useState(Math.ceil(toast.duration / 1000));
	const paused = hovered || stopped;
	const remaining = useRef(toast.duration);
	const Icon = TONE_ICON[toast.tone];

	useEffect(() => {
		if (paused) return;
		const started = Date.now();
		const timer = window.setTimeout(() => onDismiss(toast.id), remaining.current);
		const clock = window.setInterval(() => {
			setSeconds(Math.max(Math.ceil((remaining.current - (Date.now() - started)) / 1000), 1));
		}, 200);
		return () => {
			window.clearTimeout(timer);
			window.clearInterval(clock);
			remaining.current = Math.max(remaining.current - (Date.now() - started), MIN_REMAINING_MS);
		};
	}, [paused, onDismiss, toast.id]);

	return (
		<motion.li
			layout
			role={toast.tone === "error" ? "alert" : "status"}
			initial={reduce ? { opacity: 0 } : { opacity: 0, y: 22, scale: 0.96, filter: "blur(10px)" }}
			animate={reduce ? { opacity: 1 } : { opacity: 1, y: 0, scale: 1, filter: "blur(0px)" }}
			exit={
				reduce
					? { opacity: 0 }
					: { opacity: 0, x: 32, scale: 0.96, filter: "blur(8px)", transition: { duration: 0.18, ease: EASE_OUT } }
			}
			transition={STACK_SPRING}
			drag={reduce ? false : "x"}
			dragConstraints={{ left: 0, right: 0 }}
			dragElastic={{ left: 0.05, right: 0.4 }}
			onDragEnd={(_, info) => {
				if (info.offset.x > 72 || info.velocity.x > 520) onDismiss(toast.id);
			}}
			onMouseEnter={() => setHovered(true)}
			onMouseLeave={() => setHovered(false)}
			className="pointer-events-auto relative will-change-transform"
			style={{ zIndex: 20 - index }}
		>
			{/* Opaque surface: a toast must show over any ambience. */}
			<div className="grain relative overflow-hidden rounded-2xl border border-line bg-bg shadow-[0_16px_48px_rgba(0,0,0,0.55)]">
				<span aria-hidden className={cn("absolute inset-x-0 top-0 h-10 bg-gradient-to-b to-transparent", GLOW_CLASS[toast.tone])} />
				<span
					aria-hidden
					className={cn("absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent to-transparent", EDGE_CLASS[toast.tone])}
				/>

				<div className="relative flex items-start gap-3 p-3.5">
					<Icon className={cn("mt-px size-[18px] shrink-0", TONE_CLASS[toast.tone])} />

					<div className="min-w-0 flex-1">
						<p className="text-sm leading-5 font-medium text-fg">{toast.title}</p>
						<p className="mt-1 max-h-32 overflow-y-auto text-sm leading-5 break-words text-fg-muted select-text">
							{toast.details}
						</p>
					</div>

					{/* Negative margins: centered on the title's line, whatever its height. */}
					{toast.action && (
						<button
							type="button"
							onClick={() => {
								toast.action?.onClick();
								onDismiss(toast.id);
							}}
							className="-my-1.5 inline-flex h-8 shrink-0 cursor-pointer items-center rounded-full border border-line bg-surface-hover px-3.5 text-xs font-medium text-fg transition-colors hover:bg-surface-strong"
						>
							{toast.action.label}
						</button>
					)}

					<button
						type="button"
						onClick={() => onDismiss(toast.id)}
						aria-label="Dismiss"
						className="-m-1 inline-flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-fg-subtle transition-colors hover:text-fg"
					>
						<X className="size-[18px]" />
					</button>
				</div>

				{toast.action && !stopped && (
					<button
						type="button"
						onClick={() => setStopped(true)}
						className="relative block w-full cursor-pointer bg-surface px-3.5 py-2 text-left text-xs text-fg-subtle"
					>
						This message will close in {seconds} second{seconds > 1 ? "s" : ""}.{" "}
						<span className="font-medium text-fg">Click to stop.</span>
						<TimeLeft duration={toast.duration} paused={paused} className={TIMER_CLASS[toast.tone]} />
					</button>
				)}
			</div>
		</motion.li>
	);
});

/** Time-left bar, stopped together with the timer. */
function TimeLeft({ duration, paused, className }: { duration: number; paused: boolean; className: string }): ReactNode {
	return (
		<span
			aria-hidden
			className={cn("absolute inset-x-0 bottom-0 h-0.5 origin-left opacity-60", className)}
			style={{
				animation: `toast-timer ${duration}ms linear forwards`,
				animationPlayState: paused ? "paused" : "running",
			}}
		/>
	);
}
