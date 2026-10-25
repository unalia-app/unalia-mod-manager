"use client";

import { Button, Spinner } from "@modalia/ui";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { toast } from "@/components/toaster";
import { type SteamGame, removeManualGame, splitError } from "@/lib/engine";
import { forgetRecentGame, loadGames } from "@/lib/games-store";
import { forgetGameMods, useInstalled } from "@/lib/installs";

/**
 * Retire un jeu ajouté à la main : ses mods quittent son dossier, puis il
 * disparaît d'Unalia. Le jeu lui-même reste installé.
 */
export function RemoveGame({ game }: { game: SteamGame }) {
	const router = useRouter();
	const mods = useInstalled(game.appId)?.length ?? 0;
	const [confirming, setConfirming] = useState(false);
	const [removing, setRemoving] = useState(false);
	if (game.source !== "manual") return null;

	const remove = async () => {
		setRemoving(true);
		try {
			const count = await removeManualGame(game.appId);
			forgetRecentGame(game.appId);
			forgetGameMods(game.appId);
			await loadGames({ fresh: true });
			router.push("/");
			toast({
				title: `${game.name} removed from Unalia`,
				details:
					count > 0
						? `Its ${count} mod${count > 1 ? "s were" : " was"} taken out of the game folder. The game itself is still installed.`
						: "The game itself is still installed.",
			});
		} catch (error) {
			const { message, details } = splitError(error);
			toast({ title: message, details: details ?? undefined, tone: "error" });
			setRemoving(false);
			setConfirming(false);
		}
	};

	if (removing) {
		return (
			<p className="mt-2.5 flex items-center justify-end gap-1.5 text-[11px] text-fg-subtle">
				<Spinner className="size-3" />
				Removing…
			</p>
		);
	}

	if (confirming) {
		return (
			<div className="mt-2.5 flex max-w-64 flex-col items-end gap-1.5 text-right">
				<p className="text-[11px] leading-snug text-fg-subtle">
					{mods > 0
						? `Unalia takes its ${mods} mod${mods > 1 ? "s" : ""} out of the game folder and forgets the game. The game itself stays installed.`
						: "Unalia forgets the game. The game itself stays installed."}
				</p>
				<div className="flex gap-1.5">
					<Button size="sm" variant="secondary" onClick={() => setConfirming(false)}>
						Cancel
					</Button>
					<Button
						size="sm"
						variant="outline"
						className="text-danger hover:bg-danger/10"
						onClick={() => void remove()}
					>
						Remove
					</Button>
				</div>
			</div>
		);
	}

	return (
		<button
			type="button"
			onClick={() => setConfirming(true)}
			title="Take its mods out of the game folder and remove it from Unalia"
			className="mt-1.5 ml-auto flex cursor-pointer items-center gap-1 text-[11px] text-fg-subtle underline-offset-2 transition-colors hover:text-danger hover:underline"
		>
			Remove from Unalia
		</button>
	);
}
