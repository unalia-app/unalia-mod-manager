import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { currentMonitor } from "@tauri-apps/api/window";
import { useSyncExternalStore } from "react";

/**
 * Taille de l'interface : comme le zoom d'un navigateur, tout grandit
 * (textes, marges, icônes). `auto` part de la définition de l'écran : 100 %
 * jusqu'au 2K, comme Unalia dans un navigateur ; agrandi seulement sur un
 * écran 4K réglé à 100 % dans Windows, où tout serait minuscule.
 */
export type InterfaceSize = "auto" | 1 | 1.1 | 1.25 | 1.5;

export const SIZES: InterfaceSize[] = ["auto", 1, 1.1, 1.25, 1.5];

const KEY = "modalia.interfaceSize";

/**
 * Largeur de l'écran de la fenêtre, en pixels logiques. Lue auprès de Tauri :
 * sous Windows, le `screen` de la WebView reste figé sur l'écran de création
 * de la fenêtre (1920 sur un 2K, une fois la fenêtre passée d'un écran à
 * l'autre).
 */
let monitorWidth: number | null = null;

async function readMonitorWidth(): Promise<number | null> {
	try {
		const monitor = await currentMonitor();
		return monitor ? Math.round(monitor.size.width / monitor.scaleFactor) : null;
	} catch {
		return null;
	}
}

/** Facteur choisi pour cet écran. Sa largeur ne dépend pas du zoom de la page. */
export function autoScale(
	screenWidth = monitorWidth ?? (typeof window === "undefined" ? 1920 : window.screen.width),
): number {
	return screenWidth >= 3000 ? 1.5 : 1;
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

/** Applique la taille à la fenêtre (zoom de la WebView), pour l'écran où elle se trouve. */
export async function applyInterfaceSize(size = interfaceSize()) {
	if (!isTauri()) return;
	monitorWidth = (await readMonitorWidth()) ?? monitorWidth;
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
