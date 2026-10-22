import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useSyncExternalStore } from "react";

/**
 * Taille de l'interface : comme le zoom d'un navigateur, tout grandit
 * (textes, marges, icônes). `auto` part de la définition de l'écran : sur un
 * 2K à 100 %, la même taille physique qu'en 1080p.
 */
export type InterfaceSize = "auto" | 1 | 1.1 | 1.25 | 1.5;

export const SIZES: InterfaceSize[] = ["auto", 1, 1.1, 1.25, 1.5];

const KEY = "modalia.interfaceSize";

/** Facteur choisi pour cet écran. `screen.width` ne dépend pas du zoom de la page. */
export function autoScale(screenWidth = typeof window === "undefined" ? 1920 : window.screen.width): number {
	if (screenWidth >= 3000) return 1.5;
	if (screenWidth >= 2200) return 1.2;
	if (screenWidth >= 1900) return 1.1;
	return 1;
}

function read(): InterfaceSize {
	try {
		const value = localStorage.getItem(KEY);
		const parsed: InterfaceSize = value === null || value === "auto" ? "auto" : (Number(value) as InterfaceSize);
		return SIZES.includes(parsed) ? parsed : "auto";
	} catch {
		return "auto";
	}
}

let current: InterfaceSize | null = null;
const listeners = new Set<() => void>();

export function interfaceSize(): InterfaceSize {
	current ??= read();
	return current;
}

export function scaleOf(size: InterfaceSize): number {
	return size === "auto" ? autoScale() : size;
}

/** Applique la taille à la fenêtre (zoom de la WebView). */
export async function applyInterfaceSize(size = interfaceSize()) {
	if (!isTauri()) return;
	await getCurrentWebview().setZoom(scaleOf(size));
}

export function setInterfaceSize(size: InterfaceSize) {
	current = size;
	try {
		localStorage.setItem(KEY, String(size));
	} catch {
		// Stockage bloqué : le choix vaut pour cette session.
	}
	for (const listener of listeners) listener();
	void applyInterfaceSize(size);
}

export function useInterfaceSize(): InterfaceSize {
	return useSyncExternalStore(
		(listener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		interfaceSize,
		() => "auto",
	);
}
