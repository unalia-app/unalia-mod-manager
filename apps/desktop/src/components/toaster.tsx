"use client";

import { ToastStack, type ToastStackItem } from "@modalia/ui";
import { useSyncExternalStore } from "react";

/**
 * Toasts : rares, courts, jamais bloquants (SPEC §2.5). Seulement pour ce
 * qui n'a pas d'autre place à l'écran : annuler un retrait, ou une erreur
 * sans endroit où s'afficher. Ils partent seuls (le survol met le minuteur
 * en pause), ou d'un glissement, ou par la croix.
 */

type Toast = Omit<ToastStackItem, "title"> & { title: string };

type ToastInput = Omit<Toast, "id" | "duration" | "tone"> & Partial<Pick<Toast, "tone" | "duration">>;

const MAX_VISIBLE = 3;

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

	return <ToastStack toasts={list} onDismiss={dismiss} />;
}
