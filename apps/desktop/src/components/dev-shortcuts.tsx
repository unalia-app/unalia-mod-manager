"use client";

import { useEffect } from "react";

/**
 * Raccourcis de développement : ⌘R / Ctrl+R recharge la fenêtre (et rejoue
 * l'intro), Ctrl+Maj+U et Ctrl+Maj+N montrent les fenêtres de mise à jour et
 * de « What's new ». Absents des builds de production, où un rechargement
 * accidentel ferait perdre l'état de l'appli.
 */
export function DevShortcuts() {
	useEffect(() => {
		if (process.env.NODE_ENV !== "development") return;
		const onKeyDown = (event: KeyboardEvent) => {
			if (!(event.metaKey || event.ctrlKey)) return;
			const key = event.key.toLowerCase();
			if (key === "r" && !event.shiftKey) {
				event.preventDefault();
				window.location.reload();
			} else if (event.shiftKey && (key === "u" || key === "n")) {
				event.preventDefault();
				window.dispatchEvent(new Event(key === "u" ? "unalia:preview-update" : "unalia:preview-whats-new"));
			}
		};
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, []);

	return null;
}
