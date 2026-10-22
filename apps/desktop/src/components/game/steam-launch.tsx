"use client";

import { Switch, cn } from "@modalia/ui";
import { useState } from "react";

import { toast } from "@/components/toaster";
import { type SteamGame, setLaunchViaSteam, splitError } from "@/lib/engine";
import { patchGame } from "@/lib/games-store";

/**
 * « Launch through Steam », pour un jeu ajouté à la main que l'utilisateur
 * a aussi ajouté à Steam : Play passe par Steam, avec son overlay (compteur
 * FPS, captures). Sans raccourci Steam, l'option explique comment l'obtenir.
 */
export function SteamLaunch({ game }: { game: SteamGame }) {
	const [enabled, setEnabled] = useState(game.launchViaSteam);
	if (game.source !== "manual") return null;

	const change = (value: boolean) => {
		setEnabled(value);
		setLaunchViaSteam(game.appId, value).then(
			() => patchGame(game.appId, { launchViaSteam: value }),
			(error: unknown) => {
			setEnabled(!value);
				const { message } = splitError(error);
				toast({ title: message, tone: "error" });
			},
		);
	};

	return (
		<label
			className={cn("mt-2.5 flex items-center justify-end gap-2 text-[11px]", game.steamShortcut ? "text-fg-subtle" : "text-fg-faint")}
			title={
				game.steamShortcut
					? "Play starts the game from Steam, with its overlay and FPS counter"
					: "Add this game to Steam first (Games › Add a Non-Steam Game), then restart Unalia"
			}
		>
			Launch through Steam
			<Switch
				checked={game.steamShortcut && enabled}
				onChange={change}
				disabled={!game.steamShortcut}
				label="Launch through Steam"
			/>
		</label>
	);
}
