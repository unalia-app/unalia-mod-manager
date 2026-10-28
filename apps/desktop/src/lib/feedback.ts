import { useSyncExternalStore } from "react";

/**
 * Tester feedback (taken from Unalia: "beta" bubble and Help & Feedback
 * dialog). Sending isn't wired: Unalia Mod Manager has no server yet.
 * `submitFeedback` is the only place to replace.
 */

export type FeedbackType = "bug" | "feature" | "general";

export type Feedback = { type: FeedbackType; message: string };

export const MESSAGE_MIN = 10;
export const MESSAGE_MAX = 1000;

const SENT_KEY = "modalia.feedbackSent";

/** To wire to the future server. For now, nothing is sent. */
export async function submitFeedback(feedback: Feedback): Promise<void> {
	if (process.env.NODE_ENV === "development") console.info("[feedback] not sent (no backend yet)", feedback);
	await new Promise((resolve) => setTimeout(resolve, 400));
	try {
		localStorage.setItem(SENT_KEY, "1");
	} catch {
		// Storage blocked: the bubble may come back, no big deal.
	}
}

/** Feedback was already sent from this machine (the bubble doesn't come back). */
export function feedbackSent(): boolean {
	try {
		return localStorage.getItem(SENT_KEY) !== null;
	} catch {
		return false;
	}
}

// Opens the dialog from anywhere (header, bubble).
let open = false;
const listeners = new Set<() => void>();

function set(next: boolean) {
	open = next;
	for (const listener of listeners) listener();
}

export const openFeedback = () => set(true);
export const closeFeedback = () => set(false);

export function useFeedbackOpen(): boolean {
	return useSyncExternalStore(
		(listener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		() => open,
		() => false,
	);
}
