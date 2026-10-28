"use client";

import { useEffect } from "react";

/**
 * Development shortcuts: ⌘R / Ctrl+R reloads the window (and replays the
 * intro), Ctrl+Shift+U and Ctrl+Shift+N show the update and "What's new"
 * dialogs. Missing from production builds, where an accidental reload
 * would lose the app state.
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
