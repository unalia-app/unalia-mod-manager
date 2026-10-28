import { useSyncExternalStore } from "react";

import { type AvailableUpdate, checkAppUpdate, installAppUpdate, onUpdateProgress, splitError } from "@/lib/engine";
import { rememberWhatsNew } from "@/lib/whats-new";

/**
 * App update: checked at startup and from Settings, never installed
 * without a click (same as for mods).
 */
export type AppUpdateState =
	| { status: "idle" | "checking" | "upToDate" }
	| { status: "available"; update: AvailableUpdate }
	/** `progress`: from 0 to 1, `null` while the size is unknown. */
	| { status: "installing"; update: AvailableUpdate; progress: number | null }
	| { status: "error"; update: AvailableUpdate | null; message: string; details: string | null };

let state: AppUpdateState = { status: "idle" };
const listeners = new Set<() => void>();

function set(next: AppUpdateState) {
	state = next;
	for (const listener of listeners) listener();
}

export function useAppUpdate(): AppUpdateState {
	return useSyncExternalStore(
		(listener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		() => state,
		() => state,
	);
}

/** Asks the release server. Returns the update found. */
export async function checkForUpdate(): Promise<AvailableUpdate | null> {
	if (state.status === "checking" || state.status === "installing") return null;
	set({ status: "checking" });
	try {
		const update = await checkAppUpdate();
		set(update ? { status: "available", update } : { status: "upToDate" });
		return update;
	} catch (error) {
		set({ status: "error", update: null, ...splitError(error) });
		throw error;
	}
}

/**
 * Downloads and installs the update found. On success, the app closes and
 * the new version opens: the promise doesn't resolve.
 */
export async function installUpdate(): Promise<void> {
	if (state.status !== "available" && !(state.status === "error" && state.update)) return;
	const update = state.update;
	if (!update) return;
	if (previewing) return simulateInstall(update);
	set({ status: "installing", update, progress: null });
	// Read on restart by "What's new".
	rememberWhatsNew(update.version, update.notes);
	const unlisten = await onUpdateProgress(({ downloaded, total }) => {
		if (state.status !== "installing") return;
		set({ ...state, progress: total ? Math.min(downloaded / total, 1) : null });
	});
	try {
		await installAppUpdate();
	} catch (error) {
		set({ status: "error", update, ...splitError(error) });
		throw error;
	} finally {
		unlisten();
	}
}

// Dev preview (Ctrl+Shift+U): a fake update, whose install moves the bar
// forward without downloading anything.

/** Sample notes, in the `CHANGELOG.md` format. */
export const SAMPLE_NOTES = `Small fixes after the first week of the beta.

### Added

- Uninstall a mod from its page or its card.
- Uninstall Unalia from Settings > Support.

### Changed

- Toasts are easier to see, and can be swiped away.
- The game library notices games installed by Steam while the app is open.

### Fixed

- Skin packs for \`Silksong Customizer\` land in the folder the mod reads.`;

let previewing = false;

export function previewUpdate() {
	previewing = true;
	set({ status: "available", update: { version: "0.1.2", notes: SAMPLE_NOTES } });
}

async function simulateInstall(update: AvailableUpdate) {
	for (let progress = 0; progress <= 1; progress += 0.1) {
		set({ status: "installing", update, progress });
		await new Promise((resolve) => setTimeout(resolve, 200));
	}
	set({ status: "available", update });
}
