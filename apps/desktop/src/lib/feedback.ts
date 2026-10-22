import { useSyncExternalStore } from "react";

/**
 * Retours des testeurs (repris d'Unalia : bulle « beta » et fenêtre Help &
 * Feedback). L'envoi n'est pas branché : Unalia Mod Manager n'a pas encore
 * de serveur. `submitFeedback` est le seul endroit à remplacer.
 */

export type FeedbackType = "bug" | "feature" | "general";

export type Feedback = { type: FeedbackType; message: string };

export const MESSAGE_MIN = 10;
export const MESSAGE_MAX = 1000;

const SENT_KEY = "modalia.feedbackSent";

/** À brancher sur le futur serveur. Pour l'instant, rien ne part. */
export async function submitFeedback(feedback: Feedback): Promise<void> {
	if (process.env.NODE_ENV === "development") console.info("[feedback] not sent (no backend yet)", feedback);
	await new Promise((resolve) => setTimeout(resolve, 400));
	try {
		localStorage.setItem(SENT_KEY, "1");
	} catch {
		// Stockage bloqué : la bulle pourra revenir, sans gravité.
	}
}

/** Un retour a déjà été envoyé depuis cette machine (la bulle ne revient pas). */
export function feedbackSent(): boolean {
	try {
		return localStorage.getItem(SENT_KEY) !== null;
	} catch {
		return false;
	}
}

// Ouverture de la fenêtre depuis n'importe où (en-tête, bulle).
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
