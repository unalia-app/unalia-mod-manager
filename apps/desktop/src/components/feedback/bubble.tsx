"use client";

import { cn } from "@modalia/ui";
import { X } from "lucide-react";
import { type ReactNode, useCallback, useState } from "react";
import { createPortal } from "react-dom";

/** Closed or used: it never comes back. */
export function bubbleDismissed(key: string): boolean {
	try {
		return localStorage.getItem(key) !== null;
	} catch {
		// Storage blocked: show it, once per launch.
		return false;
	}
}

export function useBubble(key: string) {
	const [visible, setVisible] = useState(false);
	const [leaving, setLeaving] = useState(false);

	const show = useCallback(() => {
		setLeaving(false);
		setVisible(true);
	}, []);

	const dismiss = useCallback(() => {
		setLeaving(true);
		window.setTimeout(() => setVisible(false), 200);
		try {
			localStorage.setItem(key, "1");
		} catch {
			// Without storage, it comes back at the next launch.
		}
	}, [key]);

	return { visible, leaving, show, dismiss };
}

/** Small invitation, bottom right. */
export function Bubble({ leaving, onDismiss, children }: { leaving: boolean; onDismiss: () => void; children: ReactNode }) {
	return createPortal(
		<div
			className={cn(
				"fixed right-6 bottom-6 z-50 w-[260px] transition-[opacity,translate] duration-200 ease-out",
				leaving ? "translate-y-4 opacity-0" : "animate-fade-up",
			)}
		>
			<div className="relative overflow-hidden rounded-xl border border-white/10 bg-bg shadow-xl shadow-black/40">
				{/* Top edge line, like in Unalia */}
				<div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-accent/60 to-transparent" />
				<button
					type="button"
					onClick={onDismiss}
					aria-label="Dismiss"
					className="absolute top-2.5 right-2.5 cursor-pointer rounded-md p-1 text-white/30 transition-colors hover:bg-white/10 hover:text-white/70"
				>
					<X className="size-3.5" />
				</button>
				<div className="p-4">{children}</div>
			</div>
		</div>,
		document.body,
	);
}
