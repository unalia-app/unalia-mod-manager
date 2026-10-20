"use client";

import { cn } from "@modalia/ui";
import { CircleAlert, CircleCheck, X } from "lucide-react";
import { useEffect, useSyncExternalStore } from "react";

/**
 * Toasts : jamais de popup qui coupe le flow (SPEC §2.5). Style des toasts
 * d'Unalia : verre sombre, liseré coloré à gauche selon le ton.
 */

type Toast = {
	id: number;
	title: string;
	description?: string;
	tone: "success" | "error" | "neutral";
	action?: { label: string; onClick: () => void };
	duration: number;
};

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

function emit() {
	for (const l of listeners) l();
}

export function toast(t: Omit<Toast, "id" | "duration" | "tone"> & Partial<Pick<Toast, "tone" | "duration">>) {
	const id = nextId++;
	toasts = [...toasts.slice(-2), { tone: "neutral", duration: 5000, ...t, id }];
	emit();
	return id;
}

export function dismiss(id: number) {
	toasts = toasts.filter((t) => t.id !== id);
	emit();
}

export function Toaster() {
	const list = useSyncExternalStore(
		(l) => {
			listeners.add(l);
			return () => listeners.delete(l);
		},
		() => toasts,
		() => toasts,
	);

	return (
		<div className="pointer-events-none fixed right-5 bottom-5 z-50 flex w-80 flex-col gap-2" aria-live="polite">
			{list.map((t) => (
				<ToastItem key={t.id} toast={t} />
			))}
		</div>
	);
}

function ToastItem({ toast: t }: { toast: Toast }) {
	useEffect(() => {
		const timer = window.setTimeout(() => dismiss(t.id), t.duration);
		return () => window.clearTimeout(timer);
	}, [t.id, t.duration]);

	return (
		<div
			role="status"
			className={cn(
				"pointer-events-auto flex animate-row-in items-start gap-3 rounded-xl border border-l-2 border-line bg-black/70 p-3.5 shadow-[0_8px_32px_rgba(0,0,0,0.45)] backdrop-blur-xl",
				t.tone === "success" && "border-l-success/80",
				t.tone === "error" && "border-l-danger/80",
			)}
		>
			{t.tone === "success" && <CircleCheck className="mt-0.5 size-4 shrink-0 text-success" />}
			{t.tone === "error" && <CircleAlert className="mt-0.5 size-4 shrink-0 text-danger" />}
			<div className="min-w-0 flex-1">
				<p className="text-sm font-medium tracking-tight text-fg">{t.title}</p>
				{t.description && <p className="mt-0.5 text-xs text-fg-subtle">{t.description}</p>}
			</div>
			{t.action && (
				<button
					type="button"
					onClick={() => {
						dismiss(t.id);
						t.action?.onClick();
					}}
					className="shrink-0 cursor-pointer rounded-md bg-white px-2.5 py-1 text-xs font-medium text-bg transition-colors hover:bg-white/85"
				>
					{t.action.label}
				</button>
			)}
			<button
				type="button"
				aria-label="Dismiss"
				onClick={() => dismiss(t.id)}
				className="shrink-0 cursor-pointer rounded-md p-0.5 text-fg-faint transition-colors hover:text-fg"
			>
				<X className="size-3.5" />
			</button>
		</div>
	);
}
