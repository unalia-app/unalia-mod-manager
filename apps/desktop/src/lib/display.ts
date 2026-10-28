import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { currentMonitor } from "@tauri-apps/api/window";
import { useSyncExternalStore } from "react";

/**
 * Interface size: like a browser zoom, everything grows (text, spacing,
 * icons). `auto` starts from the screen resolution: 100% up to 2K, like
 * Unalia in a browser; enlarged only on a 4K screen set to 100% in
 * Windows, where everything would be tiny.
 */
export type InterfaceSize = "auto" | 1 | 1.1 | 1.25 | 1.5;

export const SIZES: InterfaceSize[] = ["auto", 1, 1.1, 1.25, 1.5];

const KEY = "modalia.interfaceSize";

/**
 * Width of the window's screen, in logical pixels. Read from Tauri: on
 * Windows, the WebView's `screen` stays stuck on the screen where the
 * window was created (1920 on a 2K, once the window moved from one screen
 * to another).
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

/** Factor picked for this screen. Its width doesn't depend on the page zoom. */
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

/** Applies the size to the window (WebView zoom), for the screen it is on. */
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
		// Storage blocked: the choice holds for this session.
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
