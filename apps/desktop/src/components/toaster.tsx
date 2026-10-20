"use client";

import { cn } from "@modalia/ui";
import { CircleAlert } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";

/**
 * Toasts : rares, courts, jamais bloquants (SPEC §2.5). Seulement pour ce
 * qui n'a pas d'autre place à l'écran : annuler un retrait, ou une erreur
 * sans endroit où s'afficher. Une pastille sombre en bas au centre ; pas de
 * croix, elle part seule (le survol met le minuteur en pause).
 */

type Toast = {
	id: number;
	title: string;
	/** Détail technique, replié derrière « Details ». */
	details?: string;
	tone: "neutral" | "error";
	action?: { label: string; onClick: () => void };
	duration: number;
};

type ToastInput = Omit<Toast, "id" | "duration" | "tone"> & Partial<Pick<Toast, "tone" | "duration">>;

const MAX_VISIBLE = 2;

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

function emit() {
	for (const l of listeners) l();
}

export function toast(input: ToastInput) {
	const id = nextId++;
	// Même message déjà affiché : on le remplace plutôt que de l'empiler.
	const others = toasts.filter((t) => t.title !== input.title);
	const next: Toast = { tone: "neutral", duration: 4000, ...input, id };
	toasts = [...others, next].slice(-MAX_VISIBLE);
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
		<div
			aria-live="polite"
			className="pointer-events-none fixed inset-x-0 bottom-6 z-50 flex flex-col items-center gap-2 px-6"
		>
			{list.map((t) => (
				<ToastItem key={t.id} toast={t} />
			))}
		</div>
	);
}

function ToastItem({ toast: t }: { toast: Toast }) {
	const [leaving, setLeaving] = useState(false);
	const [open, setOpen] = useState(false);
	const [paused, setPaused] = useState(false);
	const remaining = useRef(t.duration);

	useEffect(() => {
		if (paused || open) return;
		const started = Date.now();
		const timer = window.setTimeout(() => setLeaving(true), remaining.current);
		return () => {
			window.clearTimeout(timer);
			remaining.current = Math.max(remaining.current - (Date.now() - started), 800);
		};
	}, [paused, open]);

	useEffect(() => {
		if (!leaving) return;
		const timer = window.setTimeout(() => dismiss(t.id), 160);
		return () => window.clearTimeout(timer);
	}, [leaving, t.id]);

	return (
		<div
			role={t.tone === "error" ? "alert" : "status"}
			onMouseEnter={() => setPaused(true)}
			onMouseLeave={() => setPaused(false)}
			className={cn(
				"pointer-events-auto max-w-md rounded-xl border border-line-strong bg-[#18181b] px-4 py-2.5 shadow-[0_12px_40px_rgba(0,0,0,0.5)] transition-opacity duration-150",
				leaving ? "opacity-0" : "animate-row-in",
			)}
		>
			<div className="flex items-center gap-3">
				{t.tone === "error" && <CircleAlert className="size-4 shrink-0 text-danger" />}
				<p className="min-w-0 text-sm text-fg">{t.title}</p>
				{t.details && (
					<button
						type="button"
						onClick={() => setOpen((o) => !o)}
						className="shrink-0 cursor-pointer text-xs text-fg-subtle transition-colors hover:text-fg"
					>
						{open ? "Hide" : "Details"}
					</button>
				)}
				{t.action && (
					<button
						type="button"
						onClick={() => {
							t.action?.onClick();
							setLeaving(true);
						}}
						className="shrink-0 cursor-pointer text-sm font-medium text-accent transition-opacity hover:opacity-80"
					>
						{t.action.label}
					</button>
				)}
			</div>
			{open && t.details && (
				<p className="mt-2 border-t border-line pt-2 font-mono text-[11px] leading-relaxed break-words text-fg-subtle">
					{t.details}
				</p>
			)}
		</div>
	);
}
