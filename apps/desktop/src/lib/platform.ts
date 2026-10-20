import { useSyncExternalStore } from "react";

export type Platform = "mac" | "windows" | "linux";

function detect(): Platform {
	const ua = navigator.userAgent;
	if (ua.includes("Windows")) return "windows";
	if (ua.includes("Mac")) return "mac";
	return "linux";
}

/**
 * Plateforme de la fenêtre. Pendant l'export statique (pas de navigateur),
 * on suppose macOS ; la vraie valeur arrive au premier rendu côté client.
 */
export function usePlatform(): Platform {
	return useSyncExternalStore(
		() => () => {},
		detect,
		() => "mac",
	);
}
