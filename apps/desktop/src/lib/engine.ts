import { invoke, isTauri } from "@tauri-apps/api/core";
import { type UnlistenFn, listen } from "@tauri-apps/api/event";

// Miroir des types sérialisés par le moteur (serde, camelCase).
export type Engine = "unity" | "unreal" | "unknown";

/** Ce que Thunderstore sait du jeu (brique games du moteur). */
export type GameProfile = {
	id: string;
	name: string;
	community: string;
	steamAppIds: number[];
	loader: string;
	supported: boolean;
	coverUrl: string | null;
	heroUrl: string | null;
};

export type SteamGame = {
	appId: number;
	name: string;
	installDir: string;
	library: string;
	engine: Engine;
	profile: GameProfile | null;
	/** Décidé par le moteur : profil Thunderstore avec un loader géré. */
	moddable: boolean;
	/** `manual` : ajouté à la main depuis son dossier. */
	source: "steam" | "manual";
};

export type GameScan = {
	steamFound: boolean;
	games: SteamGame[];
};

export async function listGames(): Promise<GameScan> {
	// Hors de Tauri (ex. `next dev` dans un navigateur), aucun moteur n'est joignable.
	if (!isTauri()) return { steamFound: false, games: [] };
	return invoke<GameScan>("list_games");
}

/**
 * Ajoute un jeu installé hors de Steam à partir de son dossier. Le moteur le
 * reconnaît par ses fichiers ; renvoie son identifiant.
 */
export function addManualGame(path: string): Promise<number> {
	return invoke("add_manual_game", { path });
}

export function isModdable(game: SteamGame): boolean {
	return game.moddable;
}

export const ENGINE_LABEL: Record<Engine, string> = {
	unity: "Unity",
	unreal: "Unreal",
	unknown: "Unknown engine",
};

const LOADER_LABEL: Record<string, string> = {
	bepinex: "BepInEx",
	melonloader: "MelonLoader",
	"recursive-melonloader": "MelonLoader",
	shimloader: "Shimloader",
	godotml: "GodotML",
};

/** Sous-titre d'un jeu : moteur, et loader quand Thunderstore le connaît. */
export function gameSubtitle(game: SteamGame): string {
	const loader = game.profile && (LOADER_LABEL[game.profile.loader] ?? game.profile.loader);
	return loader && game.profile?.loader !== "none" ? `${ENGINE_LABEL[game.engine]} · ${loader}` : ENGINE_LABEL[game.engine];
}

export type ArtworkKind = "cover" | "hero" | "header" | "logo";

/**
 * Image du cache local de Steam, servie par le protocole `artwork` du moteur.
 * Sous Windows, la webview expose les protocoles maison en `http://<nom>.localhost`.
 */
export function artworkUrl(appId: number, kind: ArtworkKind): string {
	const windows = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
	const base = windows ? "http://artwork.localhost" : "artwork://localhost";
	return `${base}/${appId}/${kind}`;
}

const accents = new Map<number, Promise<string | null>>();

/** Couleur d'accent CSS du jeu (`null` : accent neutre). Calculée une fois. */
export function gameAccent(appId: number): Promise<string | null> {
	if (!isTauri()) return Promise.resolve(null);
	let accent = accents.get(appId);
	if (!accent) {
		accent = invoke<string | null>("game_accent", { appId }).catch(() => null);
		accents.set(appId, accent);
	}
	return accent;
}

// Mods (brique sources du moteur, format neutre SPEC §6.5).

export type ModSort = "popular" | "newest" | "updated";

export type ModSummary = {
	id: string;
	source: string;
	name: string;
	author: string;
	summary: string;
	iconUrl: string | null;
	categories: string[];
	downloads: number;
	updatedAt: string;
	pinned: boolean;
	deprecated: boolean;
};

export type ModDependency = { id: string; minVersion: string };

export type ModVersion = {
	number: string;
	publishedAt: string;
	downloadUrl: string;
	sha256: string | null;
	size: number | null;
	dependencies: ModDependency[];
};

export type Mod = Omit<ModSummary, "iconUrl" | "downloads" | "pinned"> & {
	images: string[];
	downloads: number | null;
	sourceUrl: string;
	websiteUrl: string | null;
	versions: ModVersion[];
};

export type Page<T> = { items: T[]; total: number; hasMore: boolean };

export function searchMods(community: string, text: string, sort: ModSort, page: number): Promise<Page<ModSummary>> {
	return invoke("search_mods", { community, text, sort, page });
}

/** Dépendance de la dernière version, prête à afficher (moteur : get_mod). */
export type DependencyInfo = {
	id: string;
	name: string;
	author: string;
	minVersion: string;
	/** Loader du jeu : installé à part, automatiquement. */
	loader: boolean;
};

export type ModDetails = Mod & { dependencies: DependencyInfo[] };

export function getMod(community: string, id: string): Promise<ModDetails> {
	return invoke("get_mod", { community, id });
}

/** Markdown brut de la source : à nettoyer avant tout affichage (SPEC §7). */
export function modDescription(id: string, version: string): Promise<string> {
	return invoke("mod_description", { id, version });
}

// Installation (moteur : installer, state).

export type InstalledMod = {
	game: string;
	id: string;
	name: string;
	author: string;
	version: string;
	iconUrl: string | null;
	/** Paquet loader : son dossier racine dans l'archive. */
	loaderRoot: string | null;
	enabled: boolean;
	/** Demandé par l'utilisateur (sinon installé comme dépendance). */
	explicit: boolean;
	sha256: string;
	installedAt: number;
	storeDir: string;
};

export type InstallProgress =
	| { stage: "resolving" }
	| { stage: "downloading"; index: number; total: number; name: string; received: number; size: number | null }
	| { stage: "installing"; index: number; total: number; name: string };

export type InstallEvent = { appId: number; modId: string; progress: InstallProgress };

export function installedMods(appId: number): Promise<InstalledMod[]> {
	if (!isTauri()) return Promise.resolve([]);
	return invoke("installed_mods", { appId });
}

/** Fait tout de suite, ou mis en attente parce que le jeu tourne. */
export type Outcome<T> = { status: "done"; value: T } | { status: "queued" };

export function installMod(appId: number, modId: string): Promise<Outcome<InstalledMod[]>> {
	return invoke("install_mod", { appId, modId });
}

export function setModEnabled(appId: number, modId: string, enabled: boolean): Promise<Outcome<InstalledMod>> {
	return invoke("set_mod_enabled", { appId, modId, enabled });
}

export function uninstallMod(appId: number, modId: string): Promise<Outcome<null>> {
	return invoke("uninstall_mod", { appId, modId });
}

export function restoreMod(appId: number, modId: string): Promise<InstalledMod> {
	return invoke("restore_mod", { appId, modId });
}

export function onInstallProgress(handler: (event: InstallEvent) => void): Promise<UnlistenFn> {
	if (!isTauri()) return Promise.resolve(() => {});
	return listen<InstallEvent>("install-progress", (e) => handler(e.payload));
}

/**
 * Les erreurs du moteur arrivent en deux parties : un message lisible, puis
 * après un retour à la ligne, la cause technique (à replier).
 */
export function splitError(error: unknown): { message: string; details: string | null } {
	const text = String(error);
	const [message, ...rest] = text.split("\n");
	return { message: message ?? text, details: rest.length ? rest.join("\n") : null };
}

// Lancement et file d'attente (moteur : launch).

export type PendingOp =
	| { type: "install"; modId: string; name: string }
	| { type: "setEnabled"; modId: string; enabled: boolean; name: string }
	| { type: "uninstall"; modId: string; name: string };

export type GameStatus = { appId: number; running: boolean; starting: boolean; pending: PendingOp[] };

export type PendingApplied = { appId: number; applied: number; failed: { modId: string; message: string }[] };

export function gameStatus(appId: number): Promise<GameStatus> {
	if (!isTauri()) return Promise.resolve({ appId, running: false, starting: false, pending: [] });
	return invoke("game_status", { appId });
}

export function playGame(appId: number): Promise<void> {
	return invoke("play_game", { appId });
}

export function cancelPending(appId: number, modId: string): Promise<void> {
	return invoke("cancel_pending", { appId, modId });
}

export function onGameStatus(handler: (status: GameStatus) => void): Promise<UnlistenFn> {
	if (!isTauri()) return Promise.resolve(() => {});
	return listen<GameStatus>("game-status", (e) => handler(e.payload));
}

export function onPendingApplied(handler: (event: PendingApplied) => void): Promise<UnlistenFn> {
	if (!isTauri()) return Promise.resolve(() => {});
	return listen<PendingApplied>("pending-applied", (e) => handler(e.payload));
}
