"use client";

import { TriangleAlert, X } from "lucide-react";
import { useEffect, useState } from "react";

import { type LoaderProblem, type SteamGame, loaderProblems } from "@/lib/engine";
import { useGameStatus } from "@/lib/game-status";
import { useInstalled } from "@/lib/installs";

/**
 * A loader didn't start in the last session, often after a game update. The
 * game still runs, so nothing else tells the player that these mods did nothing.
 */
export function LoaderProblemBanner({ game }: { game: SteamGame }) {
	const installed = useInstalled(game.appId);
	const running = useGameStatus(game.appId)?.running;
	const [problems, setProblems] = useState<LoaderProblem[]>([]);
	const [hidden, setHidden] = useState(false);

	useEffect(() => {
		if (!installed || running) return;
		let current = true;
		const check = () =>
			loaderProblems(game.appId).then(
				(found) => current && setProblems(found),
				() => {},
			);
		void check();
		window.addEventListener("focus", check);
		return () => {
			current = false;
			window.removeEventListener("focus", check);
		};
	}, [game.appId, installed, running]);

	if (hidden || running || problems.length === 0) return null;

	return (
		<div className="mt-6 flex animate-row-in items-start gap-4 rounded-xl border border-warning/30 bg-warning/10 px-4 py-3">
			<div className="flex min-w-0 flex-1 flex-col gap-2">
				{problems.map(({ loader, mods }) => {
					const count = mods.length;
					const names = count <= 3 ? mods.join(", ") : `${mods.slice(0, 2).join(", ")} and ${count - 2} more`;
					return (
						<span key={loader} className="flex items-start gap-2 text-sm text-fg">
							<TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" />
							<span>
								<span className="font-medium">
									{loader} didn't start in your last session of {game.name}.
								</span>{" "}
								<span className="text-fg-muted">
									{names} didn't run. This usually follows a game update: {loader} needs a new version from its author.
									Until then, the game runs without {count > 1 ? "them" : "it"}.
								</span>
							</span>
						</span>
					);
				})}
			</div>
			<button
				type="button"
				aria-label="Hide this message"
				onClick={() => setHidden(true)}
				className="cursor-pointer rounded-md p-1.5 text-fg-faint transition-colors hover:bg-white/10 hover:text-fg"
			>
				<X className="size-3.5" />
			</button>
		</div>
	);
}
