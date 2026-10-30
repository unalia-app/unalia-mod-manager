import type { InstalledMod } from "@/lib/engine";

/**
 * Shared mod list: a code to paste, with no server. A snapshot of the mods
 * the player turned on (dependencies and the loader follow by themselves,
 * as for a normal install), compressed then in base64url.
 */

/** A mod of the list. `files`: the exact Nexus files, in their order. */
export type SharedMod = { id: string; name: string; version: string; source: string; files: number[] };

/** `game`: the game's profile (the same for everyone, unlike a manually added game's id). */
export type SharedList = { game: string; gameName: string; mods: SharedMod[] };

const PREFIX = "unalia-mods:";

/** Turned on, asked for by the player: what a friend has to install. */
export function modsToShare(installed: InstalledMod[]): SharedMod[] {
	return installed
		.filter((m) => m.enabled && m.explicit && m.loaderRoot === null)
		.map((m) => ({ id: m.id, name: m.name, version: m.version, source: m.source, files: m.files }));
}

export async function encodeList(list: SharedList): Promise<string> {
	// Short keys and arrays: a shorter code to paste in a chat.
	const raw = {
		v: 1,
		g: list.game,
		n: list.gameName,
		m: list.mods.map((m) => [m.id, m.name, m.version, m.source, m.files]),
	};
	const packed = await pipe(new TextEncoder().encode(JSON.stringify(raw)), new CompressionStream("deflate-raw"));
	let binary = "";
	for (const byte of packed) binary += String.fromCharCode(byte);
	return PREFIX + btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** `null`: not a mod list code (or a damaged one). */
export async function decodeList(code: string): Promise<SharedList | null> {
	const text = code.trim();
	if (!text.startsWith(PREFIX)) return null;
	try {
		const binary = atob(text.slice(PREFIX.length).replace(/-/g, "+").replace(/_/g, "/"));
		const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
		const raw = JSON.parse(new TextDecoder().decode(await pipe(bytes, new DecompressionStream("deflate-raw"))));
		if (raw.v !== 1 || typeof raw.g !== "string" || !Array.isArray(raw.m)) return null;
		const mods: SharedMod[] = raw.m.map(([id, name, version, source, files]: unknown[]) => ({
			id: String(id),
			name: String(name),
			version: String(version),
			source: typeof source === "string" ? source : "",
			files: Array.isArray(files) ? files.filter((f): f is number => Number.isInteger(f)) : [],
		}));
		return { game: raw.g, gameName: String(raw.n ?? ""), mods };
	} catch {
		return null;
	}
}

async function pipe(
	bytes: Uint8Array<ArrayBuffer>,
	stream: CompressionStream | DecompressionStream,
): Promise<Uint8Array<ArrayBuffer>> {
	const out = new Blob([bytes]).stream().pipeThrough(stream);
	return new Uint8Array(await new Response(out).arrayBuffer());
}
