import { invoke, isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";

/**
 * Tester feedback (taken from Unalia: "beta" bubble and Help & Feedback
 * dialog). The engine sends it to the site, which forwards it to the team's
 * Discord (support.rs).
 */

export type FeedbackType = "bug" | "feature" | "general";

export type Feedback = { type: FeedbackType; message: string };

export const MESSAGE_MIN = 10;
export const MESSAGE_MAX = 1000;

const SENT_KEY = "modalia.feedbackSent";

/** Sends the message. Throws when it didn't arrive, so the user can retry. */
export async function submitFeedback(feedback: Feedback): Promise<void> {
	if (isTauri()) await invoke("send_feedback", { kind: feedback.type, message: feedback.message });
	else console.info("[feedback] not sent outside the app", feedback);
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
