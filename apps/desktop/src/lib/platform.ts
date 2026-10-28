import { useSyncExternalStore } from "react";

export type Platform = "mac" | "windows" | "linux";

function detect(): Platform {
	const ua = navigator.userAgent;
	if (ua.includes("Windows")) return "windows";
	if (ua.includes("Mac")) return "mac";
	return "linux";
}

/**
 * Platform of the window. During the static export (no browser), we assume
 * macOS; the real value arrives at the first client render.
 */
export function usePlatform(): Platform {
	return useSyncExternalStore(
		() => () => {},
		detect,
		() => "mac",
	);
}
