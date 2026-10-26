import { useEffect, useMemo, useSyncExternalStore } from "react";

import { toast } from "@/components/toaster";
import {
	type InstallProgress,
	type InstalledMod,
	type ModUpdate,
	type NexusModFile,
	cancelInstall,
	checkUpdates,
	installMod,
	installedMods,
	onInstallProgress,
	onNexusInstalled,
	restoreMod,
	revertMod,
	setModEnabled,
	splitError,
	uninstallMod,
} from "@/lib/engine";
import { openExternal } from "@/lib/open";

/**
 * Store des installations, partagé par tous les écrans : mods installés par
 * jeu, et installations en cours (une file : le moteur les traite une par
 * une, les suivantes attendent).
 */

/** `name` : nom du mod demandé, s'il est connu (sinon, celui du paquet en cours). */
export type ActiveInstall = { name?: string } & (
	| { status: "queued" }
	/** `speed` : octets par seconde (moyenne lissée), pendant un téléchargement. */
	| { status: "running"; progress: InstallProgress; speed?: number }
	/** Compte Nexus gratuit : téléchargement à confirmer sur la page ouverte. */
	| { status: "website"; url: string }
	| { status: "error"; message: string; details: string | null }
);

/** Mod Nexus à plusieurs fichiers : le choix demandé à l'utilisateur. */
export type FileChoice = { appId: number; modId: string; name?: string; files: NexusModFile[] };

/** Installation qui vient d'échouer : montrée dans une fenêtre, pas seulement sur sa carte. */
export type InstallFailure = { appId: number; modId: string; name?: string; message: string; details: string | null };

type Snapshot = {
	installed: Map<number, InstalledMod[]>;
	active: Map<string, ActiveInstall>;
	/** Mises à jour disponibles par jeu (vérifiées une fois par session). */
	updates: Map<number, ModUpdate[]>;
	choice: FileChoice | null;
	failure: InstallFailure | null;
};

let snapshot: Snapshot = { installed: new Map(), active: new Map(), updates: new Map(), choice: null, failure: null };
const listeners = new Set<() => void>();

function update(change: (draft: Snapshot) => void) {
	const draft = {
		installed: new Map(snapshot.installed),
		active: new Map(snapshot.active),
		updates: new Map(snapshot.updates),
		choice: snapshot.choice,
		failure: snapshot.failure,
	};
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

/**
 * Installations lancées par l'interface et pas encore terminées. Les autres
 * (file d'attente appliquée par le moteur à la fermeture du jeu) envoient
 * aussi leur progression, mais leur fin arrive par `pending-applied` : voir
 * `settleBackgroundInstalls`.
 */
const inFlight = new Set<string>();

let listening = false;
function listenOnce() {
	if (listening) return;
	listening = true;
	void onInstallProgress(({ appId, modId, progress }) =>
		update((s) => {
			const k = key(appId, modId);
			const name = s.active.get(k)?.name;
			s.active.set(k, { status: "running", progress, speed: measureSpeed(k, progress), name });
		}),
	);
	// Lien `nxm://` du site (bouton « Mod Manager Download ») : le moteur
	// installe seul, l'interface apprend la fin ici.
	void onNexusInstalled(({ appId, modId, name, missing, error, cancelled, website }) => {
		const k = key(appId, modId);
		// Un fichier d'un choix multiple est arrivé : la page du suivant est
		// ouverte, on attend son lien.
		if (website) {
			update((s) => {
				s.active.set(k, { status: "website", url: website, name: s.active.get(k)?.name });
			});
			toast({
				title: "Downloaded, one more file to go",
				details: "Nexus Mods opened the next file: click Slow download there.",
			});
			return;
		}
		if (cancelled) {
			update((s) => {
				s.active.delete(k);
			});
			return;
		}
		if (error) {
			const { message, details } = splitError(error);
			update((s) => {
				s.active.set(k, { status: "error", message, details, name: name ?? s.active.get(k)?.name });
				s.failure = { appId, modId, name: name ?? s.active.get(k)?.name, message, details };
			});
			return;
		}
		void refreshInstalled(appId).then(() =>
			update((s) => {
				s.active.delete(k);
				const updates = s.updates.get(appId);
				if (updates) s.updates.set(appId, updates.filter((u) => u.modId !== modId));
			}),
		);
		const required = missing[0];
		toast({
			title: `${name ?? "Mod"} installed`,
			details: missing.length
				? `It also needs ${missing.map((m) => m.name).join(", ")}. Install ${missing.length > 1 ? "them" : "it"} from Nexus Mods.`
				: undefined,
			action: required?.url
				? { label: "Open", onClick: () => void openExternal(required.url ?? "") }
				: undefined,
			duration: missing.length ? 10000 : undefined,
		});
	});
}

/** Échantillons de vitesse par installation (téléchargement en cours). */
const samples = new Map<string, { index: number; bytes: number; at: number; speed?: number }>();

/**
 * Vitesse lissée (moyenne mobile exponentielle) : assez réactive pour
 * suivre une variation, assez stable pour un temps restant lisible.
 */
function measureSpeed(k: string, progress: InstallProgress): number | undefined {
	if (progress.stage !== "downloading") {
		samples.delete(k);
		return undefined;
	}
	const now = performance.now();
	const previous = samples.get(k);
	if (!previous || previous.index !== progress.index || progress.received < previous.bytes) {
		samples.set(k, { index: progress.index, bytes: progress.received, at: now });
		return undefined;
	}
	const elapsed = (now - previous.at) / 1000;
	if (elapsed < 0.25) return previous.speed;
	const instant = (progress.received - previous.bytes) / elapsed;
	const speed = previous.speed === undefined ? instant : previous.speed * 0.7 + instant * 0.3;
	samples.set(k, { index: progress.index, bytes: progress.received, at: now, speed });
	return speed;
}

const loading = new Set<number>();

export async function refreshInstalled(appId: number) {
	const mods = await installedMods(appId);
	update((s) => {
		s.installed.set(appId, mods);
	});
}

/** Lance l'installation (sans attendre) : l'état suit dans le store. `files` : fichiers Nexus choisis. */
export function install(appId: number, modId: string, name?: string, files?: number[]): Promise<void> {
	const k = key(appId, modId);
	const current = snapshot.active.get(k);
	if (current && current.status !== "error" && current.status !== "website") return Promise.resolve();
	update((s) => {
		s.active.set(k, { status: "queued", name: name ?? current?.name });
	});
	inFlight.add(k);
	return installMod(appId, modId, files)
		.finally(() => inFlight.delete(k))
		.then(
			(outcome) =>
				update((s) => {
					// Compte Nexus gratuit : la page de téléchargement s'ouvre, le lien
					// `nxm://` reviendra avec la progression.
					if (outcome.status === "browser") {
						s.active.set(k, { status: "website", url: outcome.url, name });
						void openExternal(outcome.url);
						return;
					}
					// Plusieurs fichiers : la fenêtre de choix prend le relais.
					if (outcome.status === "choose") {
						s.active.delete(k);
						s.choice = { appId, modId, name, files: outcome.files };
						return;
					}
					// Annulée : rien n'a changé, rien à afficher.
					// Mis en attente : la file du jeu (game-status) prend le relais.
					if (outcome.status === "done") {
						s.installed.set(appId, outcome.value);
						// Installé à sa dernière version : plus de mise à jour en attente.
						const updates = s.updates.get(appId);
						if (updates) s.updates.set(appId, updates.filter((u) => u.modId !== modId));
					}
					s.active.delete(k);
				}),
			(error: unknown) =>
				update((s) => {
					const { message, details } = splitError(error);
					s.active.set(k, { status: "error", name, message, details });
					s.failure = { appId, modId, name, message, details };
				}),
		);
}

/**
 * La file d'attente du jeu vient d'être appliquée : les installations
 * qu'elle a faites sont terminées (ou ont échoué, signalé à part). On
 * efface leur progression, et on revérifie les mises à jour du jeu.
 */
export function settleBackgroundInstalls(appId: number) {
	update((s) => {
		for (const k of s.active.keys()) {
			if (k.startsWith(`${appId}:`) && !inFlight.has(k)) s.active.delete(k);
		}
		s.updates.delete(appId);
	});
}

/**
 * Annule une installation : en cours ou en file, le moteur s'arrête (le jeu
 * n'est pas touché, le déjà-téléchargé reste en cache) ; en attente du site
 * de Nexus, on oublie simplement.
 */
export async function cancel(appId: number, modId: string) {
	const active = snapshot.active.get(key(appId, modId));
	if (active?.status === "queued" || active?.status === "running") {
		const stopped = await cancelInstall(appId, modId);
		if (stopped) return;
	}
	clearError(appId, modId);
}

/** Choix des fichiers en cours (fenêtre ouverte), sinon `null`. */
export function useFileChoice(): FileChoice | null {
	return useSyncExternalStore(subscribe, () => snapshot.choice, () => null);
}

/** Installe les fichiers choisis, dans cet ordre (ou ferme la fenêtre sans rien faire). */
export function settleFileChoice(files: number[] | null) {
	const choice = snapshot.choice;
	update((s) => {
		s.choice = null;
	});
	if (choice && files?.length) void install(choice.appId, choice.modId, choice.name, files);
}

export function useInstallFailure(): InstallFailure | null {
	return useSyncExternalStore(subscribe, () => snapshot.failure, () => null);
}

/** Ferme la fenêtre d'échec. L'erreur reste sur la carte du mod. */
export function dismissFailure() {
	update((s) => {
		s.failure = null;
	});
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

/** Jeu retiré d'Unalia : ses mods et mises à jour en mémoire partent aussi. */
export function forgetGameMods(appId: number) {
	update((s) => {
		s.installed.delete(appId);
		s.updates.delete(appId);
	});
}

/** Revient à une version précédente du mod, gardée dans le stockage. */
export async function revert(appId: number, modId: string, pkg: string): Promise<InstalledMod> {
	const reverted = await revertMod(appId, modId, pkg);
	await refreshInstalled(appId);
	recheckUpdates(appId);
	return reverted;
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

/** Installations du jeu en cours, en file ou à confirmer (pas les échecs). */
export function useActiveInstalls(appId: number): [string, ActiveInstall][] {
	const active = useSyncExternalStore(subscribe, () => snapshot.active, () => emptyActive);
	return useMemo(
		() =>
			[...active]
				.filter(([k, a]) => k.startsWith(`${appId}:`) && a.status !== "error")
				.map(([k, a]) => [k.slice(`${appId}:`.length), a] as [string, ActiveInstall]),
		[active, appId],
	);
}

const emptyActive = new Map<string, ActiveInstall>();

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
			return (progress.index + part * 0.8) / progress.total;
		}
		case "extracting":
			return (progress.index + 0.8 + (progress.done / Math.max(progress.size, 1)) * 0.15) / progress.total;
		case "installing":
			return (progress.index + 0.92) / progress.total;
	}
}

const checking = new Set<number>();

/** Mises à jour disponibles pour le jeu (`undefined` tant que la vérification tourne). */
export function useUpdates(appId: number): ModUpdate[] | undefined {
	const list = useSyncExternalStore(subscribe, () => snapshot.updates.get(appId), () => undefined);
	useEffect(() => {
		if (snapshot.updates.has(appId) || checking.has(appId)) return;
		checking.add(appId);
		void checkUpdates(appId)
			.then(
				(updates) =>
					update((s) => {
						s.updates.set(appId, updates);
					}),
				() => {},
			)
			.finally(() => checking.delete(appId));
	}, [appId]);
	return list;
}

export function useModUpdate(appId: number, modId: string): ModUpdate | undefined {
	return useUpdates(appId)?.find((u) => u.modId === modId);
}

/** Mises à jour à revérifier (ex. après une mise à jour du jeu). */
export function recheckUpdates(appId: number) {
	update((s) => {
		s.updates.delete(appId);
	});
}

/** Met à jour plusieurs mods, l'un après l'autre (le moteur les traite en file). */
export function updateAll(appId: number, updates: ModUpdate[]) {
	const names = new Map((snapshot.installed.get(appId) ?? []).map((m) => [m.id, m.name]));
	for (const u of updates) void install(appId, u.modId, names.get(u.modId));
}
