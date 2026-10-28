"use client";

import { type ReactNode, useLayoutEffect, useRef, useState } from "react";

import { cn } from "./cn";

export type SegmentedItem<T extends string> = {
	value: T;
	label: ReactNode;
};

/**
 * Exclusive choice with an indicator that slides to the active option.
 * - `tabs`: screen tabs, underline in the game's accent;
 * - `pills`: small selector (sort, filters), glass pill, at the height of a
 *   compact `SearchField` (h-10) to line up in a toolbar.
 */
export function Segmented<T extends string>({
	items,
	value,
	onChange,
	variant = "pills",
	label,
	className,
}: {
	items: SegmentedItem<T>[];
	value: T;
	onChange: (value: T) => void;
	variant?: "tabs" | "pills";
	/** Accessible name of the group. */
	label: string;
	className?: string;
}) {
	const refs = useRef(new Map<T, HTMLButtonElement>());
	const [indicator, setIndicator] = useState<{ x: number; width: number } | null>(null);

	useLayoutEffect(() => {
		const el = refs.current.get(value);
		if (!el) return;
		const measure = () => setIndicator({ x: el.offsetLeft, width: el.offsetWidth });
		measure();
		// Labels change width when the font finishes loading.
		const observer = new ResizeObserver(measure);
		observer.observe(el);
		return () => observer.disconnect();
	}, [value, items]);

	const tabs = variant === "tabs";

	return (
		<div
			role="tablist"
			aria-label={label}
			className={cn(
				"relative inline-flex items-center",
				tabs ? "gap-6 border-b border-line" : "h-10 gap-0.5 rounded-lg border border-line bg-surface p-1",
				className,
			)}
		>
			{indicator && (
				<span
					aria-hidden
					// Only element animated in width: small, and the width follows the label.
					style={{ transform: `translateX(${indicator.x}px)`, width: indicator.width }}
					className={cn(
						"absolute left-0 transition-[transform,width] duration-300 ease-out",
						tabs ? "-bottom-px h-0.5 rounded-full bg-accent" : "inset-y-1 rounded-md bg-surface-strong",
					)}
				/>
			)}
			{items.map((item) => {
				const active = item.value === value;
				return (
					<button
						key={item.value}
						ref={(el) => {
							if (el) refs.current.set(item.value, el);
							else refs.current.delete(item.value);
						}}
						type="button"
						role="tab"
						aria-selected={active}
						onClick={() => onChange(item.value)}
						className={cn(
							"relative z-10 cursor-pointer font-medium transition-colors duration-200",
							tabs ? "pb-3 text-sm" : "h-full rounded-md px-3.5 text-xs",
							active ? "text-fg" : "text-fg-subtle hover:text-fg-muted",
						)}
					>
						{item.label}
					</button>
				);
			})}
		</div>
	);
}
