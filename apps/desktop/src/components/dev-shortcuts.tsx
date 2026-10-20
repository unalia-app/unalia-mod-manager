"use client";

import { useEffect } from "react";

/**
 * Raccourcis de développement : ⌘R / Ctrl+R recharge la fenêtre (et rejoue
 * l'intro). Absents des builds de production, où un rechargement
 * accidentel ferait perdre l'état de l'appli.
 */
export function DevShortcuts() {
	useEffect(() => {
		if (process.env.NODE_ENV !== "development") return;
		const onKeyDown = (event: KeyboardEvent) => {
			if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "r") {
				event.preventDefault();
				window.location.reload();
			}
		};
		window.addEventListener("keydown", onKeyDown);
		return () => window.removeEventListener("keydown", onKeyDown);
	}, []);

	return null;
}
