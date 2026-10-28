"use client";
// beui.dev/components/motion/animated-toast-stack, adapted to the
// @modalia/ui tokens: each toast holds its timer (paused on hover and while
// the details are open), with a time-left bar.

import { AnimatePresence, motion, type Transition, useReducedMotion } from "motion/react";
import { Check, CircleAlert, Info, type LucideIcon, X } from "lucide-react";
import { memo, type ReactNode, useEffect, useRef, useState } from "react";

import { cn } from "./cn";

export type ToastTone = "neutral" | "info" | "success" | "error";

export type ToastStackItem = {
	id: number;
	title: ReactNode;
	tone: ToastTone;
	/** Technical detail, folded behind "Details". */
	details?: string;
	action?: { label: string; onClick: () => void };
	/** En millisecondes. */
	duration: number;
};

const EASE_OUT = [0.16, 1, 0.3, 1] as const;

const STACK_SPRING: Transition = { type: "spring", stiffness: 420, damping: 34, mass: 0.75 };

const TONE_ICON: Record<ToastTone, LucideIcon> = {
	neutral: Info,
	info: Info,
	success: Check,
	error: CircleAlert,
};

const TONE_CLASS: Record<ToastTone, string> = {
	neutral: "bg-surface-strong text-fg-muted",
	info: "bg-info/15 text-info",
	success: "bg-success/15 text-success",
	error: "bg-danger/15 text-danger",
};

const TIMER_CLASS: Record<ToastTone, string> = {
	neutral: "bg-accent",
	info: "bg-info",
	success: "bg-success",
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
	const [open, setOpen] = useState(false);
	const paused = hovered || open;
	const remaining = useRef(toast.duration);
	const Icon = TONE_ICON[toast.tone];

	useEffect(() => {
		if (paused) return;
		const started = Date.now();
		const timer = window.setTimeout(() => onDismiss(toast.id), remaining.current);
		return () => {
			window.clearTimeout(timer);
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
			<div className="relative overflow-hidden rounded-2xl border border-line-strong bg-[#18181b]/95 p-3 shadow-[0_16px_48px_rgba(0,0,0,0.55)] backdrop-blur-xl">
				<div className="flex items-start gap-3">
					<span
						className={cn("mt-0.5 inline-flex size-7 shrink-0 items-center justify-center rounded-full", TONE_CLASS[toast.tone])}
					>
						<Icon className="size-3.5" strokeWidth={2.5} />
					</span>

					<div className="min-w-0 flex-1 pt-1">
						<p className="text-sm leading-5 font-medium text-fg">{toast.title}</p>

						{(toast.action || toast.details) && (
							<div className="mt-2 flex flex-wrap items-center gap-2">
								{toast.action && (
									<button
										type="button"
										onClick={() => {
											toast.action?.onClick();
											onDismiss(toast.id);
										}}
										className="inline-flex h-7 cursor-pointer items-center rounded-full bg-accent px-3 text-xs font-semibold text-bg transition-[filter] hover:brightness-110"
									>
										{toast.action.label}
									</button>
								)}
								{toast.details && (
									<button
										type="button"
										aria-expanded={open}
										onClick={() => setOpen((o) => !o)}
										className="inline-flex h-7 cursor-pointer items-center rounded-full bg-surface-strong px-3 text-xs font-medium text-fg-muted transition-colors hover:text-fg"
									>
										{open ? "Hide details" : "Details"}
									</button>
								)}
							</div>
						)}

						<AnimatePresence initial={false}>
							{open && toast.details && (
								<motion.p
									initial={reduce ? { opacity: 0 } : { opacity: 0, height: 0 }}
									animate={reduce ? { opacity: 1 } : { opacity: 1, height: "auto" }}
									exit={reduce ? { opacity: 0 } : { opacity: 0, height: 0 }}
									transition={{ duration: 0.22, ease: EASE_OUT }}
									className="overflow-hidden"
								>
									<span className="mt-2 block border-t border-line pt-2 font-mono text-[11px] leading-relaxed break-words text-fg-subtle select-text">
										{toast.details}
									</span>
								</motion.p>
							)}
						</AnimatePresence>
					</div>

					<button
						type="button"
						onClick={() => onDismiss(toast.id)}
						aria-label="Dismiss"
						className="inline-flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-full text-fg-subtle transition-colors hover:bg-surface-strong hover:text-fg"
					>
						<X className="size-3.5" />
					</button>
				</div>

				<TimeLeft duration={toast.duration} paused={paused} className={TIMER_CLASS[toast.tone]} />
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
