"use client";

import { ToastStack, type ToastStackItem, type ToastTone } from "@modalia/ui";
import { useSyncExternalStore } from "react";

/**
 * Toasts: rare, short, never blocking. Only for what has no other place on
 * screen: undoing a removal, or an error with nowhere to show. They leave
 * by themselves (hover pauses the timer), with a swipe, or with the
 * cross.
 */

type Toast = Omit<ToastStackItem, "title"> & { title: string };

/** Always a line under the title: only an error, with nothing more to say, can leave it to `ERROR_HELP`. */
type ToastInput = Pick<Toast, "title" | "action"> &
	Partial<Pick<Toast, "duration">> &
	({ tone?: Exclude<ToastTone, "error">; details: string } | { tone: "error"; details?: string });

const ERROR_HELP = "Try again. If it keeps failing, tell us in Help & Feedback.";

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
	const next: Toast = { tone: "neutral", duration: 4000, ...input, details: input.details ?? ERROR_HELP, id };
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
