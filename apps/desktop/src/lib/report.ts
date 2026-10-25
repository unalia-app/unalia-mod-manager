import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";

import { autoScale, interfaceSize } from "@/lib/display";
import { diagnostics, gameSubtitle, installedMods, isModdable, nexusStatus, sourceName } from "@/lib/engine";
import { loadGames } from "@/lib/games-store";

/**
 * Rapport à coller dans un signalement de bug (Markdown) : version, système,
 * jeux moddables et leurs mods, fin du journal. Ni nom de compte, ni clé,
 * ni chemin personnel.
 */
export async function buildReport(): Promise<string> {
	const [version, system, scan, nexus] = await Promise.all([
		isTauri() ? getVersion() : Promise.resolve("dev"),
		diagnostics(),
		loadGames().catch(() => null),
		nexusStatus().catch(() => null),
	]);
	const size = interfaceSize();
	const lines = [
		"## Unalia Mod Manager report",
		"",
		`- Version: ${version}`,
		`- System: ${system.os}${system.arch ? ` (${system.arch})` : ""}`,
		`- Interface size: ${size === "auto" ? `auto (${Math.round(autoScale() * 100)}%)` : `${Math.round(size * 100)}%`}`,
		`- Nexus Mods: ${nexusLine(nexus)}`,
		`- Steam: ${scan ? (scan.steamFound ? "found" : "not found") : "couldn't read the library"}`,
		"",
		"### Games",
		"",
	];

	const games = scan?.games.filter(isModdable) ?? [];
	if (games.length === 0) lines.push("No moddable game found.");
	for (const game of games) {
		const mods = await installedMods(game.appId).catch(() => null);
		const off = mods?.filter((m) => !m.enabled).length ?? 0;
		const count = mods === null ? "mods unreadable" : `${mods.length} mods${off ? ` (${off} off)` : ""}`;
		const origin = game.source === "manual" ? "added manually" : "Steam";
		lines.push(`- ${game.name} (${game.appId}): ${gameSubtitle(game)}, ${sourceName(game)}, ${origin}, ${count}`);
		for (const mod of mods ?? []) {
			lines.push(`  - ${mod.name} ${mod.version}${mod.enabled ? "" : " (off)"}${mod.explicit ? "" : " (dependency)"}`);
		}
	}
	const others = (scan?.games.length ?? 0) - games.length;
	if (others > 0) lines.push(`- ${others} other games not supported yet`);

	lines.push("", "### Recent log", "", "```", system.log || "(empty)", "```");
	return lines.join("\n");
}

function nexusLine(status: Awaited<ReturnType<typeof nexusStatus>> | null): string {
	if (!status) return "unknown";
	if (status.offline) return "linked, can't be reached";
	if (!status.account) return "not linked";
	return status.account.premium ? "linked, Premium" : "linked, free account";
}
