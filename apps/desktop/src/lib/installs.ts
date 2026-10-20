import { useEffect, useSyncExternalStore } from "react";

import {
	type InstallProgress,
	type InstalledMod,
	installMod,
	installedMods,
	onInstallProgress,
	restoreMod,
	setModEnabled,
	splitError,
	uninstallMod,
} from "@/lib/engine";

/**
 * Store des installations, partagé par tous les écrans : mods installés par
 * jeu, et installations en cours (une file : le moteur les traite une par
 * une, les suivantes attendent).
 */

export type ActiveInstall =
	| { status: "queued" }
	| { status: "running"; progress: InstallProgress }
	| { status: "error"; message: string; details: string | null };

type Snapshot = {
	installed: Map<number, InstalledMod[]>;
	active: Map<string, ActiveInstall>;
};

let snapshot: Snapshot = { installed: new Map(), active: new Map() };
const listeners = new Set<() => void>();

function update(change: (draft: Snapshot) => void) {
	const draft = { installed: new Map(snapshot.installed), active: new Map(snapshot.active) };
	change(draft);
	snapshot = draft;
	for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
	listeners.add(listener);
	listenOnce();
	return () => listeners.delete(listener);
}

const key = (appId: number, modId: string) => `${appId}:${modId}`;

let listening = false;
function listenOnce() {
	if (listening) return;
	listening = true;
	void onInstallProgress(({ appId, modId, progress }) =>
		update((s) => {
			s.active.set(key(appId, modId), { status: "running", progress });
		}),
	);
}

const loading = new Set<number>();

export async function refreshInstalled(appId: number) {
	const mods = await installedMods(appId);
	update((s) => {
		s.installed.set(appId, mods);
	});
}

/** Lance l'installation (sans attendre) : l'état suit dans le store. */
export function install(appId: number, modId: string): Promise<void> {
	const k = key(appId, modId);
	const current = snapshot.active.get(k);
	if (current && current.status !== "error") return Promise.resolve();
	update((s) => {
		s.active.set(k, { status: "queued" });
	});
	return installMod(appId, modId).then(
		(outcome) =>
			update((s) => {
				// Mis en attente : la file du jeu (game-status) prend le relais.
				if (outcome.status === "done") s.installed.set(appId, outcome.value);
				s.active.delete(k);
			}),
		(error: unknown) =>
			update((s) => {
				s.active.set(k, { status: "error", ...splitError(error) });
			}),
	);
}

export function clearError(appId: number, modId: string) {
	update((s) => {
		s.active.delete(key(appId, modId));
	});
}

/** Renvoie `queued` si le jeu tourne : le changement attend sa fermeture. */
export async function toggle(appId: number, modId: string, enabled: boolean): Promise<"done" | "queued"> {
	// Optimiste : l'interrupteur bouge tout de suite (SPEC §3.5 « instantané »).
	const apply = (value: boolean) =>
		update((s) => {
			const list = s.installed.get(appId);
			if (list) s.installed.set(appId, list.map((m) => (m.id === modId ? { ...m, enabled: value } : m)));
		});
	apply(enabled);
	try {
		const outcome = await setModEnabled(appId, modId, enabled);
		// En attente : l'état réel ne change qu'à la fermeture du jeu ;
		// l'interface montre le changement prévu à partir de la file.
		if (outcome.status === "queued") apply(!enabled);
		return outcome.status;
	} catch (error) {
		apply(!enabled);
		throw error;
	}
}

export async function remove(appId: number, modId: string): Promise<"done" | "queued"> {
	const outcome = await uninstallMod(appId, modId);
	if (outcome.status === "done") await refreshInstalled(appId);
	return outcome.status;
}

export async function restore(appId: number, modId: string) {
	await restoreMod(appId, modId);
	await refreshInstalled(appId);
}

/** Mods installés du jeu (`undefined` pendant le premier chargement). */
export function useInstalled(appId: number): InstalledMod[] | undefined {
	const list = useSyncExternalStore(subscribe, () => snapshot.installed.get(appId), () => undefined);
	useEffect(() => {
		if (snapshot.installed.has(appId) || loading.has(appId)) return;
		loading.add(appId);
		void refreshInstalled(appId).finally(() => loading.delete(appId));
	}, [appId]);
	return list;
}

export function useActiveInstall(appId: number, modId: string): ActiveInstall | undefined {
	return useSyncExternalStore(subscribe, () => snapshot.active.get(key(appId, modId)), () => undefined);
}

/** Avancement global d'une installation, de 0 à 1. */
export function overallProgress(progress: InstallProgress): number {
	switch (progress.stage) {
		case "resolving":
			return 0.03;
		case "downloading": {
			const part = progress.size ? progress.received / progress.size : 0.5;
			return (progress.index + part * 0.85) / progress.total;
		}
		case "installing":
			return (progress.index + 0.92) / progress.total;
	}
}
