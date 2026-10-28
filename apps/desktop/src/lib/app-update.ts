import { useSyncExternalStore } from "react";

import { type AvailableUpdate, checkAppUpdate, installAppUpdate, onUpdateProgress, splitError } from "@/lib/engine";
import { rememberWhatsNew } from "@/lib/whats-new";

/**
 * Mise à jour de l'appli : cherchée au démarrage et depuis Settings, jamais
 * installée sans un clic (comme pour les mods, SPEC §3.5).
 */
export type AppUpdateState =
	| { status: "idle" | "checking" | "upToDate" }
	| { status: "available"; update: AvailableUpdate }
	/** `progress` : de 0 à 1, `null` tant que la taille est inconnue. */
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

/** Interroge le serveur des releases. Renvoie la mise à jour trouvée. */
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
 * Télécharge et installe la mise à jour trouvée. En cas de succès, l'appli
 * se ferme et la nouvelle version s'ouvre : la promesse ne se résout pas.
 */
export async function installUpdate(): Promise<void> {
	if (state.status !== "available" && !(state.status === "error" && state.update)) return;
	const update = state.update;
	if (!update) return;
	if (previewing) return simulateInstall(update);
	set({ status: "installing", update, progress: null });
	// Lu à la relance par « What's new ».
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

// Aperçu en dev (Ctrl+Maj+U) : une fausse mise à jour, dont l'installation
// fait avancer la barre sans rien télécharger.

/** Notes d'exemple, au format de `CHANGELOG.md`. */
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
