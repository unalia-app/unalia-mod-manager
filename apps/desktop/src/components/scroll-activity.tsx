"use client";

import { useEffect } from "react";

/** Délai après le dernier défilement avant d'effacer la barre. */
const HIDE_AFTER = 1000;

/**
 * Pose `data-scrolling` sur <html> pendant un défilement, n'importe où
 * dans la page (capture) : les barres de défilement ne s'affichent qu'à ce
 * moment-là (voir `styles.css` de @modalia/ui).
 */
export function ScrollActivity() {
	useEffect(() => {
		const root = document.documentElement;
		let timer: number | undefined;
		const onScroll = () => {
			root.setAttribute("data-scrolling", "");
			window.clearTimeout(timer);
			timer = window.setTimeout(() => root.removeAttribute("data-scrolling"), HIDE_AFTER);
		};
		document.addEventListener("scroll", onScroll, { capture: true, passive: true });
		return () => {
			document.removeEventListener("scroll", onScroll, { capture: true });
			window.clearTimeout(timer);
		};
	}, []);

	return null;
}
