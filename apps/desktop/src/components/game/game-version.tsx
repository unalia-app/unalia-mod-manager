"use client";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@modalia/ui";
import { useState } from "react";

import { toast } from "@/components/toaster";
import { type SteamGame, setGameVersion, splitError } from "@/lib/engine";
import { patchGame } from "@/lib/games-store";
import { recheckUpdates } from "@/lib/installs";

/**
 * Game version, for people who went back to an old one (Steam, GOG): mods
 * then install in their version published before the next one, and
 * frameworks in their release from back then.
 */
/** Value of the latest version in the menu (no setting). */
const LATEST = "latest";

export function GameVersion({ game }: { game: SteamGame }) {
	const versions = game.profile?.versions ?? [];
	const [value, setValue] = useState(game.gameVersion ?? LATEST);
	if (versions.length < 2) return null;
	const latest = versions[versions.length - 1];

	const change = (next: string) => {
		const previous = value;
		setValue(next);
		const version = next === LATEST ? null : next;
		setGameVersion(game.appId, version).then(
			async () => {
				await patchGame(game.appId, { gameVersion: version });
				recheckUpdates(game.appId);
				const after = versions[versions.findIndex((v) => v.name === next) + 1];
				toast(
					version && after
						? {
								title: `Mods for ${game.name} ${version}`,
								details: `Unalia installs the files published before ${after.name}. Mods you installed for a newer version show up as updates.`,
							}
						: { title: `Mods for the latest ${game.name}`, details: "Updates bring every mod to its newest version." },
				);
			},
			(error: unknown) => {
				setValue(previous);
				const { message, details } = splitError(error);
				toast({ title: message, details: details ?? undefined, tone: "error" });
			},
		);
	};

	return (
		<div
			className="mt-2.5 flex items-center justify-end gap-2 text-[11px] text-fg-subtle"
			title="Rolled the game back? Unalia installs the mods and frameworks made for your version."
		>
			Game version
			<Select value={value} onValueChange={change} className="w-32">
				<SelectTrigger className="h-7 px-2.5 py-0 text-xs">
					<SelectValue />
				</SelectTrigger>
				<SelectContent>
					<SelectItem value={LATEST} className="text-xs">{`${latest.name} (latest)`}</SelectItem>
					{versions
						.slice(0, -1)
						.reverse()
						.map((v) => (
							<SelectItem key={v.name} value={v.name} className="text-xs">
								{v.name}
							</SelectItem>
						))}
				</SelectContent>
			</Select>
		</div>
	);
}
