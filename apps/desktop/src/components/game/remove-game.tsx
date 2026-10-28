"use client";

import { Button, Dialog, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@modalia/ui";
import { LogOut } from "lucide-react";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { toast } from "@/components/toaster";
import { type SteamGame, removeManualGame, splitError } from "@/lib/engine";
import { forgetRecentGame, loadGames } from "@/lib/games-store";
import { forgetGameMods, useInstalled } from "@/lib/installs";

/**
 * Removes a manually added game: its mods leave its folder, then it
 * disappears from Unalia. The game itself stays installed.
 */
export function RemoveGame({ game }: { game: SteamGame }) {
	const router = useRouter();
	const mods = useInstalled(game.appId)?.length ?? 0;
	const [open, setOpen] = useState(false);
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
			setOpen(false);
		}
	};

	return (
		<>
			<Button
				variant="outline"
				size="sm"
				leftIcon={<LogOut className="size-3.5" />}
				title="Take its mods out of the game folder and remove it from Unalia"
				onClick={() => setOpen(true)}
			>
				Remove from Unalia
			</Button>

			<Dialog open={open} onClose={removing ? undefined : () => setOpen(false)}>
				<DialogHeader>
					<DialogTitle>Remove {game.name} from Unalia?</DialogTitle>
					<DialogDescription>
						{mods > 0
							? `Unalia takes its ${mods} mod${mods > 1 ? "s" : ""} out of the game folder and forgets the game.`
							: "Unalia forgets the game."}{" "}
						The game itself stays installed, with your saves. You can add it again any time.
					</DialogDescription>
				</DialogHeader>
				<DialogFooter>
					<Button variant="secondary" disabled={removing} onClick={() => setOpen(false)}>
						Cancel
					</Button>
					<Button variant="danger" isLoading={removing} onClick={() => void remove()}>
						Remove
					</Button>
				</DialogFooter>
			</Dialog>
		</>
	);
}
