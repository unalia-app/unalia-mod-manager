"use client";

import { ToastStack, type ToastStackItem } from "@modalia/ui";
import { useSyncExternalStore } from "react";

/**
 * Toasts: rare, short, never blocking. Only for what has no other place on
 * screen: undoing a removal, or an error with nowhere to show. They leave
 * by themselves (hover pauses the timer), with a swipe, or with the
 * cross.
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
	// Same message already shown: replace it rather than stack it.
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
