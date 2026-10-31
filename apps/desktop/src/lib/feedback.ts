import { invoke, isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";

import { buildReport } from "@/lib/report";

/**
 * Tester feedback (taken from Unalia: "beta" bubble and Help & Feedback
 * dialog). The engine sends it to the site, which forwards it to the team's
 * Discord (support.rs).
 */

export type FeedbackType = "bug" | "feature" | "general";

/** `attach`: with the report of `buildReport`, for a bug. */
export type Feedback = { type: FeedbackType; message: string; attach: boolean };

export const MESSAGE_MIN = 10;
export const MESSAGE_MAX = 1000;

/** Same bound as the site (apps/site/app/api/feedback/route.ts). */
const REPORT_MAX = 100_000;

const SENT_KEY = "modalia.feedbackSent";

/** The report, its middle removed if too long: it starts with the versions and ends with the log. */
async function report(): Promise<string | null> {
	const text = await buildReport().catch(() => null);
	if (text === null || text.length <= REPORT_MAX) return text;
	const half = REPORT_MAX / 2 - 32;
	return `${text.slice(0, half)}\n\n(cut, report too long)\n\n${text.slice(-half)}`;
}

/** Sends the message. Throws when it didn't arrive, so the user can retry. */
export async function submitFeedback(feedback: Feedback): Promise<void> {
	if (isTauri()) {
		await invoke("send_feedback", {
			kind: feedback.type,
			message: feedback.message,
			// A report that can't be built doesn't stop the message.
			report: feedback.attach ? await report() : null,
		});
	} else console.info("[feedback] not sent outside the app", feedback);
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
