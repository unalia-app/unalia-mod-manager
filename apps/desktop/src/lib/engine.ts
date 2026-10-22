import { invoke, isTauri } from "@tauri-apps/api/core";
import { type UnlistenFn, listen } from "@tauri-apps/api/event";

// Miroir des types sérialisés par le moteur (serde, camelCase).
export type Engine = "unity" | "unreal" | "unknown";

/** Ce que Modalia sait du jeu : schéma Thunderstore ou profil écrit à la main (brique games). */
export type GameProfile = {
	id: string;
	name: string;
	/** Communauté Thunderstore, ou `nexus:<domaine>` pour un jeu Nexus Mods. */
	community: string;
	steamAppIds: number[];
	loader: string;
	supported: boolean;
	coverUrl: string | null;
	heroUrl: string | null;
	/** Jeu dont les mods viennent de Nexus Mods. */
	nexus: { domain: string; gameId: number } | null;
};

export type SteamGame = {
	appId: number;
	name: string;
	installDir: string;
	library: string;
	engine: Engine;
	/** Dernière mise à jour du jeu (secondes Unix), si connue. */
	lastUpdated: number | null;
	profile: GameProfile | null;
	/** Décidé par le moteur : profil connu, avec un loader géré. */
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
	redengine: "REDengine",
	bepisloader: "BepisLoader",
	umm: "Unity Mod Manager",
	northstar: "Northstar",
};

/** Sous-titre d'un jeu : moteur, et loader quand Modalia le connaît. */
export function gameSubtitle(game: SteamGame): string {
	const loader = game.profile && game.profile.loader !== "none" ? (LOADER_LABEL[game.profile.loader] ?? game.profile.loader) : null;
	if (!loader) return ENGINE_LABEL[game.engine];
	// Moteur maison (REDengine…) : le loader suffit.
	return game.engine === "unknown" ? loader : `${ENGINE_LABEL[game.engine]} · ${loader}`;
}

/** Nom de la source des mods du jeu, pour les textes. */
export function sourceName(game: SteamGame | null): string {
	return game?.profile?.nexus ? "Nexus Mods" : "Thunderstore";
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

export type ModSort = "popular" | "downloads" | "newest" | "updated";

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
	/** Lien de don de l'auteur : 100 % du don lui revient. */
	donationUrl: string | null;
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
	/** Installée avec le mod. Sinon (prérequis Nexus, compte gratuit), à installer soi-même. */
	automatic: boolean;
};

/** `direct` : s'installe sans passer par le site de la source (frameworks Nexus pris sur GitHub). */
export type ModDetails = Mod & { dependencies: DependencyInfo[]; direct: boolean };

export function getMod(community: string, id: string): Promise<ModDetails> {
	return invoke("get_mod", { community, id });
}

/** Markdown brut de la source : à nettoyer avant tout affichage (SPEC §7). */
export function modDescription(community: string, id: string, version: string): Promise<string> {
	return invoke("mod_description", { community, id, version });
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
	| { stage: "extracting"; index: number; total: number; name: string; done: number; size: number }
	| { stage: "installing"; index: number; total: number; name: string };

export type InstallEvent = { appId: number; modId: string; progress: InstallProgress };

export function installedMods(appId: number): Promise<InstalledMod[]> {
	if (!isTauri()) return Promise.resolve([]);
	return invoke("installed_mods", { appId });
}

/** Fait tout de suite, ou mis en attente parce que le jeu tourne. */
export type Outcome<T> = { status: "done"; value: T } | { status: "queued" };

/**
 * Une installation peut aussi demander une confirmation sur le site de la
 * source (Nexus, compte gratuit) : le lien `nxm://` revient ensuite à Modalia.
 */
export type InstallOutcome =
	| Outcome<InstalledMod[]>
	| { status: "browser"; url: string }
	| { status: "cancelled" };

export function installMod(appId: number, modId: string): Promise<InstallOutcome> {
	return invoke("install_mod", { appId, modId });
}

/** Arrête une installation en cours ou en file (le jeu n'est pas touché). */
export function cancelInstall(appId: number, modId: string): Promise<boolean> {
	return invoke("cancel_install", { appId, modId });
}

/** Téléchargement interrompu (appli quittée, réseau perdu), reprenable. */
export type PausedDownload = { appId: number; modId: string; name: string; received: number; total: number | null };

export function pausedDownloads(appId: number): Promise<PausedDownload[]> {
	if (!isTauri()) return Promise.resolve([]);
	return invoke("paused_downloads", { appId });
}

/** Jette un téléchargement en pause et ce qu'il avait déjà reçu. */
export function discardDownload(appId: number, modId: string): Promise<void> {
	return invoke("discard_download", { appId, modId });
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

export type ModUpdate = { modId: string; current: string; latest: string };

export function checkUpdates(appId: number): Promise<ModUpdate[]> {
	if (!isTauri()) return Promise.resolve([]);
	return invoke("check_updates", { appId });
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
	const [first, ...rest] = text.split("\n");
	const message = first ?? text;
	// Les messages Rust commencent par une minuscule ; l'interface suit la
	// casse de phrase.
	return {
		message: message.charAt(0).toUpperCase() + message.slice(1),
		details: rest.length ? rest.join("\n") : null,
	};
}

// Lancement et file d'attente (moteur : launch).

/** `ready` : installation déjà téléchargée pendant la partie. */
export type PendingOp = { name: string; ready: boolean } & (
	| { type: "install"; modId: string }
	| { type: "setEnabled"; modId: string; enabled: boolean }
	| { type: "uninstall"; modId: string }
);

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

// Nexus Mods (moteur : sources::nexus).

export type NexusAccount = { name: string; premium: boolean; avatarUrl: string | null };

/** `sso` : connexion en un clic disponible (Modalia approuvé par Nexus). */
export type NexusStatus = { account: NexusAccount | null; sso: boolean };

/** Prérequis Nexus que Modalia n'installe pas seul. */
export type NexusRequirement = { modId: number | null; name: string; url: string | null };

/** Fin d'une installation lancée par un lien `nxm://` du site. */
export type NexusInstalled = {
	appId: number;
	modId: string;
	/** Annulée par l'utilisateur : rien à signaler. */
	cancelled: boolean;
	name: string | null;
	missing: NexusRequirement[];
	error: string | null;
};

export function nexusStatus(): Promise<NexusStatus> {
	if (!isTauri()) return Promise.resolve({ account: null, sso: false });
	return invoke("nexus_status");
}

/** Connexion en un clic : la page d'autorisation de Nexus s'ouvre, la clé revient seule. */
export function nexusLogin(): Promise<NexusAccount> {
	return invoke("nexus_login");
}

export function nexusConnect(apiKey: string): Promise<NexusAccount> {
	return invoke("nexus_connect", { apiKey });
}

export function nexusDisconnect(): Promise<void> {
	return invoke("nexus_disconnect");
}

export function onNexusInstalled(handler: (event: NexusInstalled) => void): Promise<UnlistenFn> {
	if (!isTauri()) return Promise.resolve(() => {});
	return listen<NexusInstalled>("nexus-installed", (e) => handler(e.payload));
}

/** Page Nexus d'un mod (`nexus-<id>`). */
export function nexusModUrl(domain: string, modId: string, tab?: "files"): string {
	const id = modId.replace(/^nexus-/, "");
	return `https://www.nexusmods.com/${domain}/mods/${id}${tab ? `?tab=${tab}` : ""}`;
}
