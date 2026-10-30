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
 * Install store, shared by every screen: installed mods per game, and
 * running installs (a queue: the engine handles them one by one, the next
 * ones wait).
 */

/** `name`: name of the requested mod, if known (else, the current package's). */
export type ActiveInstall = { name?: string } & (
	| { status: "queued" }
	/** `speed`: bytes per second (smoothed average), during a download. */
	| { status: "running"; progress: InstallProgress; speed?: number }
	/** Free Nexus account: download to confirm on the opened page. */
	| { status: "website"; url: string }
	| { status: "error"; message: string; details: string | null }
);

/** Nexus mod with several files: the choice asked of the user. */
export type FileChoice = { appId: number; modId: string; name?: string; files: NexusModFile[] };

/** Archive with one folder per variant: the choice asked of the user. */
export type VariantChoice = { appId: number; modId: string; name?: string; variants: string[] };

/** Install that just failed: shown in a dialog, not only on its card. */
export type InstallFailure = { appId: number; modId: string; name?: string; message: string; details: string | null };

type Snapshot = {
	installed: Map<number, InstalledMod[]>;
	active: Map<string, ActiveInstall>;
	/** Available updates per game (checked once per session). */
	updates: Map<number, ModUpdate[]>;
	choice: FileChoice | null;
	variantChoice: VariantChoice | null;
	failure: InstallFailure | null;
};

let snapshot: Snapshot = {
	installed: new Map(),
	active: new Map(),
	updates: new Map(),
	choice: null,
	variantChoice: null,
	failure: null,
};
const listeners = new Set<() => void>();

function update(change: (draft: Snapshot) => void) {
	const draft = {
		installed: new Map(snapshot.installed),
		active: new Map(snapshot.active),
		updates: new Map(snapshot.updates),
		choice: snapshot.choice,
		variantChoice: snapshot.variantChoice,
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
 * Installs started by the UI and not finished yet. The others (queue
 * applied by the engine when the game closes) also send their progress,
 * but their end comes through `pending-applied`: see
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
	// `nxm://` link from the site ("Mod Manager Download" button): the
	// engine installs by itself, the UI learns about the end here.
	void onNexusInstalled(({ appId, modId, name, missing, error, cancelled, website, variants }) => {
		const k = key(appId, modId);
		// Downloaded, but the archive holds several variants: the picker
		// takes over, the install resumes from the cached archive.
		if (variants) {
			update((s) => {
				const known = s.active.get(k)?.name;
				s.active.delete(k);
				s.variantChoice = { appId, modId, name: name ?? known, variants };
			});
			return;
		}
		// A file of a multiple choice arrived: the next one's page is open,
		// we wait for its link.
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
		const them = missing.length > 1 ? "them" : "it";
		// Outside Nexus (ReShade): its own website.
		const from = missing.every((m) => m.modId !== null)
			? "Nexus Mods"
			: missing.length > 1
				? "their websites"
				: "its website";
		toast({
			title: `${name ?? "Mod"} installed`,
			details: missing.length
				? `It also needs ${missing.map((m) => m.name).join(", ")}. Install ${them} from ${from}.`
				: undefined,
			action: required?.url
				? { label: "Open", onClick: () => void openExternal(required.url ?? "") }
				: undefined,
			duration: missing.length ? 10000 : undefined,
		});
	});
}

/** Speed samples per install (download in progress). */
const samples = new Map<string, { index: number; bytes: number; at: number; speed?: number }>();

/**
 * Smoothed speed (exponential moving average): reactive enough to follow
 * a change, stable enough for a readable time left.
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

// Source picked for each install: a retry (or the file picker) keeps it.
const sources = new Map<string, string>();

/**
 * Starts the install (without waiting): the state follows in the store.
 * `files`: picked Nexus files. `source`: picked in Discover (`hexium`).
 * `variant`: picked folder of an archive with variants.
 */
export function install(
	appId: number,
	modId: string,
	name?: string,
	files?: number[],
	source?: string,
	variant?: string,
): Promise<void> {
	const k = key(appId, modId);
	if (source) sources.set(k, source);
	const current = snapshot.active.get(k);
	if (current && current.status !== "error" && current.status !== "website") return Promise.resolve();
	update((s) => {
		s.active.set(k, { status: "queued", name: name ?? current?.name });
	});
	inFlight.add(k);
	return installMod(appId, modId, files, sources.get(k), variant)
		.finally(() => inFlight.delete(k))
		.then(
			(outcome) =>
				update((s) => {
					// Free Nexus account: the download page opens, the `nxm://`
					// link will come back with the progress.
					if (outcome.status === "browser") {
						s.active.set(k, { status: "website", url: outcome.url, name });
						void openExternal(outcome.url);
						return;
					}
					// Several files: the picker dialog takes over.
					if (outcome.status === "choose") {
						s.active.delete(k);
						s.choice = { appId, modId, name, files: outcome.files };
						return;
					}
					if (outcome.status === "chooseVariant") {
						s.active.delete(k);
						s.variantChoice = { appId, modId, name, variants: outcome.variants };
						return;
					}
					// Canceled: nothing changed, nothing to show.
					// Queued: the game's queue (game-status) takes over.
					if (outcome.status === "done") {
						s.installed.set(appId, outcome.value);
						// Installed at its latest version: no pending update anymore.
						const updates = s.updates.get(appId);
						if (updates) s.updates.set(appId, updates.filter((u) => u.modId !== modId));
					}
					s.active.delete(k);
					sources.delete(k);
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
 * The game's queue was just applied: the installs it ran are finished (or
 * failed, reported separately). We clear their progress, and check the
 * game's updates again.
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
 * Cancels an install: running or queued, the engine stops (the game is not
 * touched, what was downloaded stays cached). Waiting for the Nexus site,
 * its `nxm://` link may already be in the engine, queued behind another
 * install: it is stopped too, else we simply forget it.
 */
export async function cancel(appId: number, modId: string) {
	const active = snapshot.active.get(key(appId, modId));
	if (active && active.status !== "error") {
		const stopped = await cancelInstall(appId, modId);
		// Running: its end clears it. Waiting its turn: it only stops when
		// its turn comes, so it leaves the list now.
		if (stopped && active.status === "running") return;
	}
	clearError(appId, modId);
}

/** File picking in progress (dialog open), else `null`. */
export function useFileChoice(): FileChoice | null {
	return useSyncExternalStore(subscribe, () => snapshot.choice, () => null);
}

/** Installs the picked files, in this order (or closes the dialog without doing anything). */
export function settleFileChoice(files: number[] | null) {
	const choice = snapshot.choice;
	update((s) => {
		s.choice = null;
	});
	if (choice && files?.length) void install(choice.appId, choice.modId, choice.name, files);
}

/** Variant picking in progress (dialog open), else `null`. */
export function useVariantChoice(): VariantChoice | null {
	return useSyncExternalStore(subscribe, () => snapshot.variantChoice, () => null);
}

/** Installs the picked variant (or closes the dialog without doing anything). */
export function settleVariantChoice(variant: string | null) {
	const choice = snapshot.variantChoice;
	update((s) => {
		s.variantChoice = null;
	});
	if (choice && variant) void install(choice.appId, choice.modId, choice.name, undefined, undefined, variant);
}

export function useInstallFailure(): InstallFailure | null {
	return useSyncExternalStore(subscribe, () => snapshot.failure, () => null);
}

/** Closes the failure dialog. The error stays on the mod card. */
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

/** Returns `queued` if the game is running: the change waits for it to close. */
export async function toggle(appId: number, modId: string, enabled: boolean): Promise<"done" | "queued"> {
	// Optimistic: the switch moves right away.
	const apply = (value: boolean) =>
		update((s) => {
			const list = s.installed.get(appId);
			if (list) s.installed.set(appId, list.map((m) => (m.id === modId ? { ...m, enabled: value } : m)));
		});
	apply(enabled);
	try {
		const outcome = await setModEnabled(appId, modId, enabled);
		// Queued: the real state only changes when the game closes; the UI
		// shows the planned change from the queue.
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

/** Game removed from Unalia: its in-memory mods and updates go too. */
export function forgetGameMods(appId: number) {
	update((s) => {
		s.installed.delete(appId);
		s.updates.delete(appId);
	});
}

/** Goes back to a previous version of the mod, kept in the storage. */
export async function revert(appId: number, modId: string, pkg: string): Promise<InstalledMod> {
	const reverted = await revertMod(appId, modId, pkg);
	await refreshInstalled(appId);
	recheckUpdates(appId);
	return reverted;
}

/** Installed mods of the game (`undefined` during the first load). */
export function useInstalled(appId: number): InstalledMod[] | undefined {
	const list = useSyncExternalStore(subscribe, () => snapshot.installed.get(appId), () => undefined);
	useEffect(() => {
		if (snapshot.installed.has(appId) || loading.has(appId)) return;
		loading.add(appId);
		void refreshInstalled(appId).finally(() => loading.delete(appId));
	}, [appId]);
	return list;
}

/** The game's installs running, queued or to confirm (not the failures). */
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

/** Overall progress of an install, from 0 to 1. */
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

/** Available updates for the game (`undefined` while the check runs). */
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

/** Updates to check again (e.g. after a game update). */
export function recheckUpdates(appId: number) {
	update((s) => {
		s.updates.delete(appId);
	});
}

/** Updates several mods, one after the other (the engine queues them). */
export function updateAll(appId: number, updates: ModUpdate[]) {
	const names = new Map((snapshot.installed.get(appId) ?? []).map((m) => [m.id, m.name]));
	for (const u of updates) void install(appId, u.modId, names.get(u.modId));
}
