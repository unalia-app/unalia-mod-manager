import { invoke, isTauri } from "@tauri-apps/api/core";
import { type UnlistenFn, listen } from "@tauri-apps/api/event";

// Mirror of the types serialized by the engine (serde, camelCase).
export type Engine = "unity" | "unreal" | "unknown";

/** What Modalia knows about the game: Thunderstore schema or hand-written profile (games block). */
export type GameProfile = {
	id: string;
	name: string;
	/** Thunderstore community, or `nexus:<domain>` for a Nexus Mods game. */
	community: string;
	steamAppIds: number[];
	loader: string;
	supported: boolean;
	coverUrl: string | null;
	heroUrl: string | null;
	/** Game whose mods come from Nexus Mods. */
	nexus: { domain: string; gameId: number } | null;
	/** Major versions of the game, oldest to newest (ISO release date). */
	versions: { name: string; released: string }[];
};

export type SteamGame = {
	appId: number;
	name: string;
	installDir: string;
	library: string;
	engine: Engine;
	/** Last game update (Unix seconds), if known. */
	lastUpdated: number | null;
	profile: GameProfile | null;
	/** Decided by the engine: known profile, with a supported loader. */
	moddable: boolean;
	/** `manual`: added by hand from its folder. */
	source: "steam" | "manual";
	/** Manually added game that the user also added to Steam (non-Steam game). */
	steamShortcut: boolean;
	/** Play goes through Steam, for its overlay (FPS counter…). */
	launchViaSteam: boolean;
	/** Old version of the game the user stayed on; `null`: the latest. */
	gameVersion: string | null;
	/** Anti-cheat found in the game (e.g. `Easy Anti-Cheat`): mods allowed, online ban possible. */
	antiCheat: string | null;
};

export type GameScan = {
	steamFound: boolean;
	games: SteamGame[];
};

export async function listGames(): Promise<GameScan> {
	// Outside Tauri (e.g. `next dev` in a browser), no engine is reachable.
	if (!isTauri()) return { steamFound: false, games: [] };
	return invoke<GameScan>("list_games");
}

/**
 * Adds the game to Steam (non-Steam game, with its name and images). Steam
 * is closed then opened again if it was open.
 */
export function addToSteam(appId: number): Promise<void> {
	return invoke("add_to_steam", { appId });
}

/** Ticks or unticks "Launch through Steam" (manually added game). */
export function setLaunchViaSteam(appId: number, enabled: boolean): Promise<void> {
	return invoke("set_launch_via_steam", { appId, enabled });
}

/**
 * Installed game version, after going back (`null`: the latest). Installs
 * and updates then take the files published for it.
 */
export function setGameVersion(appId: number, version: string | null): Promise<void> {
	return invoke("set_game_version", { appId, version });
}

/**
 * Adds a game installed outside Steam from its folder. The engine
 * recognizes it by its files; returns its id.
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
	bethesda: "Creation Engine",
	redengine: "REDengine",
	unreal: "Paks & UE4SS",
	bepisloader: "BepisLoader",
	umm: "Unity Mod Manager",
	northstar: "Northstar",
};

/** Subtitle of a game: engine, and loader when Modalia knows it. */
export function gameSubtitle(game: SteamGame): string {
	const loader = game.profile && game.profile.loader !== "none" ? (LOADER_LABEL[game.profile.loader] ?? game.profile.loader) : null;
	if (!loader) return ENGINE_LABEL[game.engine];
	// In-house engine (REDengine, Creation Engine…): the loader is enough.
	return game.engine === "unknown" ? loader : `${ENGINE_LABEL[game.engine]} · ${loader}`;
}

/** Name of the game's mod source, for copy. */
export function sourceName(game: SteamGame | null): string {
	return game?.profile?.nexus ? "Nexus Mods" : "Thunderstore";
}

export type ArtworkKind = "cover" | "hero" | "header" | "logo";

/**
 * Image from Steam's local cache, served by the engine's `artwork` protocol.
 * On Windows, the webview exposes custom protocols as `http://<name>.localhost`.
 */
export function artworkUrl(appId: number, kind: ArtworkKind): string {
	const windows = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
	const base = windows ? "http://artwork.localhost" : "artwork://localhost";
	return `${base}/${appId}/${kind}`;
}

const accents = new Map<number, Promise<string | null>>();

/** CSS accent color of the game (`null`: neutral accent). Computed once. */
export function gameAccent(appId: number): Promise<string | null> {
	if (!isTauri()) return Promise.resolve(null);
	let accent = accents.get(appId);
	if (!accent) {
		accent = invoke<string | null>("game_accent", { appId }).catch(() => null);
		accents.set(appId, accent);
	}
	return accent;
}

// Mods (engine sources block, neutral format).

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
	/** The author's donation link: 100% of the donation goes to them. */
	donationUrl: string | null;
	versions: ModVersion[];
};

export type Page<T> = { items: T[]; total: number; hasMore: boolean };

/** Display name of a mod's source (`ModSummary.source`). */
export const SOURCE_LABEL: Record<string, string> = {
	hexium: "Hexium",
	nexus: "Nexus Mods",
	thunderstore: "Thunderstore",
};

/** Thunderstore communities also hosted on Hexium (e.g. `valheim`). */
export function hexiumCommunities(): Promise<string[]> {
	if (!isTauri()) return Promise.resolve([]);
	return invoke("hexium_communities");
}

export function searchMods(community: string, text: string, sort: ModSort, page: number): Promise<Page<ModSummary>> {
	return invoke("search_mods", { community, text, sort, page });
}

/** Dependency of the latest version, ready to display (engine: get_mod). */
export type DependencyInfo = {
	id: string;
	name: string;
	author: string;
	minVersion: string;
	/** The game's loader: installed apart, automatically. */
	loader: boolean;
	/** Installed with the mod. Otherwise (Nexus requirement, free account), to install yourself. */
	automatic: boolean;
};

/** `direct`: installs without going through the source's site (Nexus frameworks taken from GitHub). */
export type ModDetails = Mod & { dependencies: DependencyInfo[]; direct: boolean };

export function getMod(community: string, id: string): Promise<ModDetails> {
	return invoke("get_mod", { community, id });
}

/** Raw Markdown from the source: sanitize it before any display. */
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
	/** Loader package: its root folder in the archive. */
	loaderRoot: string | null;
	enabled: boolean;
	/** Requested by the user (else installed as a dependency). */
	explicit: boolean;
	sha256: string;
	installedAt: number;
	/** Storage package that holds its files. */
	package: string;
	/** Installed Nexus files, in the order they are placed. */
	files: number[];
	/** Source it was installed from (`thunderstore`, `hexium`), empty: the game's source. */
	source: string;
	/** Previous versions, kept to go back without downloading again, the most recent first. */
	previousVersions: PreviousVersion[];
};

export type PreviousVersion = {
	version: string;
	package: string;
	/** Secondes Unix. */
	installedAt: number;
	replacedAt: number;
};

/** A file of a Nexus mod (variant, option…). */
export type NexusModFile = {
	fileId: number;
	name: string;
	version: string;
	/** `MAIN`, `OPTIONAL`, `UPDATE`, `MISCELLANEOUS`… */
	category: string;
	primary: boolean;
	size: number | null;
	/** Secondes Unix. */
	uploadedAt: number;
	description: string;
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

/** Done right away, or queued because the game is running. */
export type Outcome<T> = { status: "done"; value: T } | { status: "queued" };

/**
 * An install can also ask for a confirmation on the source's site (Nexus,
 * free account): the `nxm://` link then comes back to Modalia.
 */
export type InstallOutcome =
	| Outcome<InstalledMod[]>
	| { status: "browser"; url: string }
	| { status: "cancelled" }
	/** Nexus mod with several files: to pick, the advised one first. */
	| { status: "choose"; files: NexusModFile[] };

/**
 * `files`: picked Nexus files, placed in this order. `source`: picked in
 * Discover (`hexium`); without it, an update keeps the mod's source.
 */
export function installMod(appId: number, modId: string, files?: number[], source?: string): Promise<InstallOutcome> {
	return invoke("install_mod", { appId, modId, files: files ?? null, source: source ?? null });
}

/** Stops a running or queued install (the game is not touched). */
export function cancelInstall(appId: number, modId: string): Promise<boolean> {
	return invoke("cancel_install", { appId, modId });
}

/** Interrupted download (app closed, network lost), resumable. */
export type PausedDownload = { appId: number; modId: string; name: string; received: number; total: number | null };

export function pausedDownloads(appId: number): Promise<PausedDownload[]> {
	if (!isTauri()) return Promise.resolve([]);
	return invoke("paused_downloads", { appId });
}

/** Throws away a paused download and what it already received. */
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

/** Goes back to a previous version (`package`, one of the `previousVersions`). */
export function revertMod(appId: number, modId: string, pkg: string): Promise<InstalledMod> {
	return invoke("revert_mod", { appId, modId, package: pkg });
}

/** Enabled mods deleted from the game folder (game reinstalled, files verified): their names. */
export function missingMods(appId: number): Promise<string[]> {
	if (!isTauri()) return Promise.resolve([]);
	return invoke("missing_mods", { appId });
}

/** Places the deleted mods again from the storage, downloading nothing again. Returns their count. */
export function putBackMods(appId: number): Promise<number> {
	return invoke("put_back_mods", { appId });
}

/** Uninstalled game whose mods Unalia still keeps. `size`: bytes freed by forgetting it. */
export type ForgottenGame = { appId: number; name: string; mods: number; size: number };

export function forgottenGames(): Promise<ForgottenGame[]> {
	if (!isTauri()) return Promise.resolve([]);
	return invoke("forgotten_games");
}

export function forgetGame(appId: number): Promise<void> {
	return invoke("forget_game", { appId });
}

/** Removes a manually added game: its mods leave its folder, the game stays installed. Returns the number of removed mods. */
export function removeManualGame(appId: number): Promise<number> {
	return invoke("remove_manual_game", { appId });
}

/** Mod location placed in the game by another tool (by hand, Vortex…). */
export type ForeignFiles = {
	/** Relative to the game folder, with `/`. */
	path: string;
	dir: boolean;
	/** Files that don't come from Unalia (0: empty folder left behind). */
	files: number;
	bytes: number;
	/**
	 * Surely a loader leftover: ticked by default. Otherwise (common DLL
	 * name without its loader), it may serve the game: unticked.
	 */
	sure: boolean;
};

export type ResetPreview = { mods: number; foreign: ForeignFiles[] };

export type ResetResult = { mods: number; foreign: number; left: string[] };

/** What a reset would remove from the game. */
export function resetPreview(appId: number): Promise<ResetPreview> {
	return invoke("reset_preview", { appId });
}

/**
 * Removes Unalia's mods and sends the `foreign` locations (ticked) left by
 * other tools to the Recycle Bin. The game's files stay.
 */
export function resetGame(appId: number, foreign: string[]): Promise<ResetResult> {
	return invoke("reset_game", { appId, foreign });
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
 * Engine errors come in two parts: a readable message, then after a line
 * break, the technical cause (to fold).
 */
export function splitError(error: unknown): { message: string; details: string | null } {
	const text = String(error);
	const [first, ...rest] = text.split("\n");
	const message = first ?? text;
	// Rust messages start with a lowercase letter; the UI follows sentence
	// case.
	return {
		message: message.charAt(0).toUpperCase() + message.slice(1),
		details: rest.length ? rest.join("\n") : null,
	};
}

// Lancement et file d'attente (moteur : launch).

/** `ready`: install already downloaded during the session. */
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

/** Stops waiting for a starting game (launch abandoned in its launcher). */
export function cancelStarting(appId: number): Promise<void> {
	return invoke("cancel_starting", { appId });
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

/**
 * `sso`: one-click login available (Modalia approved by Nexus).
 * `offline`: key saved but Nexus unreachable, `account` is the last recognized account.
 */
export type NexusStatus = { account: NexusAccount | null; sso: boolean; offline: boolean };

/** Nexus requirement that Modalia doesn't install by itself. */
export type NexusRequirement = { modId: number | null; name: string; url: string | null };

/** End of an install started by an `nxm://` link from the site. */
export type NexusInstalled = {
	appId: number;
	modId: string;
	/** Canceled by the user: nothing to report. */
	cancelled: boolean;
	name: string | null;
	missing: NexusRequirement[];
	error: string | null;
	/** Next file of a multiple choice: its page, already open in the browser. */
	website: string | null;
};

export function nexusStatus(): Promise<NexusStatus> {
	if (!isTauri()) return Promise.resolve({ account: null, sso: false, offline: false });
	return invoke("nexus_status");
}

/** One-click login: Nexus's authorization page opens, the key comes back by itself. */
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

/** Nexus page of a mod (`nexus-<id>`). */
export function nexusModUrl(domain: string, modId: string, tab?: "files"): string {
	const id = modId.replace(/^nexus-/, "");
	return `https://www.nexusmods.com/${domain}/mods/${id}${tab ? `?tab=${tab}` : ""}`;
}

// Support (moteur : support.rs).

/** System, and end of the app log (personal paths and keys masked). */
export type Diagnostics = { os: string; arch: string; log: string };

export function diagnostics(): Promise<Diagnostics> {
	if (!isTauri()) return Promise.resolve({ os: navigator.userAgent, arch: "", log: "" });
	return invoke("diagnostics");
}

export function openLogsFolder(): Promise<void> {
	return invoke("open_logs_folder");
}

/** `app`: the app's GPL v3. `thirdParty`: the dependencies' licenses. */
export function openLicense(license: "app" | "thirdParty"): Promise<void> {
	return invoke("open_license", { license });
}

/** False in dev and outside Windows: no uninstaller. */
export function canUninstall(): Promise<boolean> {
	if (!isTauri()) return Promise.resolve(false);
	return invoke("can_uninstall");
}

/** Opens the Windows uninstaller, then the app closes. */
export function uninstallApp(): Promise<void> {
	return invoke("uninstall_app");
}

// App updates (engine: app_update).

export type AvailableUpdate = { version: string; notes: string | null };

export type UpdateProgress = { downloaded: number; total: number | null };

/** A newer version of the app, or `null`: it is up to date. */
export function checkAppUpdate(): Promise<AvailableUpdate | null> {
	if (!isTauri()) return Promise.resolve(null);
	return invoke("check_app_update");
}

/** Installs the update found: the app closes and restarts. */
export function installAppUpdate(): Promise<void> {
	return invoke("install_app_update");
}

export function onUpdateProgress(handler: (progress: UpdateProgress) => void): Promise<UnlistenFn> {
	if (!isTauri()) return Promise.resolve(() => {});
	return listen<UpdateProgress>("app-update-progress", (e) => handler(e.payload));
}
