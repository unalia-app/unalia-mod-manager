"use client";

import { Button } from "@modalia/ui";
import { Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { toast } from "@/components/toaster";
import { type ForgottenGame, forgetGame, forgottenGames, splitError } from "@/lib/engine";
import { bytes } from "@/lib/format";

/** Stockage : les mods des jeux désinstallés, à libérer. */
export function Storage() {
	const [games, setGames] = useState<ForgottenGame[] | null>(null);
	const [busy, setBusy] = useState<number | null>(null);

	useEffect(() => {
		let current = true;
		forgottenGames().then(
			(list) => current && setGames(list),
			() => current && setGames([]),
		);
		return () => {
			current = false;
		};
	}, []);

	const forget = async (game: ForgottenGame) => {
		setBusy(game.appId);
		try {
			await forgetGame(game.appId);
			setGames((list) => list?.filter((g) => g.appId !== game.appId) ?? null);
			toast({ title: `${bytes(game.size)} freed`, details: `${game.name}'s mods are gone from Unalia.` });
		} catch (error) {
			const { message, details } = splitError(error);
			toast({ title: message, details: details ?? undefined, tone: "error" });
		} finally {
			setBusy(null);
		}
	};

	return (
		<>
			<div className="space-y-1">
				<h1 className="text-2xl font-semibold tracking-tight">Storage</h1>
				<p className="text-sm text-fg-subtle">Unalia keeps a copy of every mod, so turning one on or off is instant.</p>
			</div>

			<div className="rounded-xl border border-line bg-white/[0.03] p-5 sm:p-6">
				<h3 className="text-sm font-semibold text-fg">Games you uninstalled</h3>
				<p className="mt-0.5 max-w-xl text-xs leading-relaxed text-fg-subtle">
					Their mods are still in Unalia's storage: reinstall the game and they come back in one click. Free the space if
					you're done with it. A game on an unplugged drive shows up here too: plug it back in first.
				</p>

				{games && games.length === 0 && <p className="mt-4 text-xs text-fg-faint">Nothing to free up.</p>}

				{games && games.length > 0 && (
					<ul className="mt-4 divide-y divide-line rounded-lg border border-line">
						{games.map((game) => (
							<li key={game.appId} className="flex items-center gap-4 px-4 py-3">
								<div className="min-w-0 flex-1">
									<p className="truncate text-sm text-fg">{game.name}</p>
									<p className="text-xs text-fg-subtle">
										{game.mods} mod{game.mods > 1 ? "s" : ""} - {bytes(game.size)}
									</p>
								</div>
								<Button
									size="sm"
									variant="outline"
									leftIcon={<Trash2 className="size-3.5" />}
									disabled={busy !== null}
									onClick={() => void forget(game)}
								>
									Free {bytes(game.size)}
								</Button>
							</li>
						))}
					</ul>
				)}
			</div>
		</>
	);
}
