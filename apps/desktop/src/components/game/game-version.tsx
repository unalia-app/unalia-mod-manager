"use client";

import { useState } from "react";

import { toast } from "@/components/toaster";
import { type SteamGame, setGameVersion, splitError } from "@/lib/engine";
import { patchGame } from "@/lib/games-store";
import { recheckUpdates } from "@/lib/installs";

/**
 * Version du jeu, pour qui est revenu à une ancienne (Steam, GOG) : les
 * mods s'installent alors dans leur version publiée avant la suivante, et
 * les frameworks dans leur release d'alors.
 */
export function GameVersion({ game }: { game: SteamGame }) {
	const versions = game.profile?.versions ?? [];
	const [value, setValue] = useState(game.gameVersion ?? "");
	if (versions.length < 2) return null;
	const latest = versions[versions.length - 1];

	const change = (next: string) => {
		const previous = value;
		setValue(next);
		const version = next === "" ? null : next;
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
		<label
			className="mt-2.5 flex items-center justify-end gap-2 text-[11px] text-fg-subtle"
			title="Rolled the game back? Unalia installs the mods and frameworks made for your version."
		>
			Game version
			<select
				value={value}
				onChange={(event) => change(event.target.value)}
				className="h-6 cursor-pointer rounded-md border border-line bg-surface px-1.5 text-[11px] text-fg transition-colors hover:border-line-strong focus-visible:outline-2 focus-visible:outline-accent"
			>
				<option value="">{latest.name} (latest)</option>
				{versions
					.slice(0, -1)
					.reverse()
					.map((v) => (
						<option key={v.name} value={v.name}>
							{v.name}
						</option>
					))}
			</select>
		</label>
	);
}
