"use client";

import { Button, Spinner, cn } from "@modalia/ui";
import { Play } from "lucide-react";

import { play, useGameStatus } from "@/lib/game-status";

/**
 * « Play » (SPEC §3.6) : lance le jeu avec ses mods actifs. `compact` : la
 * version de l'en-tête, toujours à portée pendant le défilement.
 */
export function PlayButton({ appId, compact = false }: { appId: number; compact?: boolean }) {
	const status = useGameStatus(appId);
	const size = compact ? "sm" : "lg";

	if (status?.running) {
		return (
			<div
				className={cn(
					"flex items-center gap-2.5 rounded-md border border-accent/30 bg-accent/10 font-medium text-accent",
					compact ? "h-8 px-3 text-xs" : "h-10 px-4 text-sm",
				)}
			>
				<span className="relative flex size-2">
					<span className="absolute inline-flex size-full animate-ping rounded-full bg-accent opacity-60" />
					<span className="relative inline-flex size-2 rounded-full bg-accent" />
				</span>
				Playing
			</div>
		);
	}
	if (status?.starting) {
		return (
			<Button size={size} variant="outline" disabled className="disabled:opacity-100">
				<span className="flex items-center gap-2">
					<Spinner className="size-3.5" />
					Starting…
				</span>
			</Button>
		);
	}
	return (
		<Button
			size={size}
			leftIcon={<Play className={cn("fill-current", compact ? "size-3.5" : "size-4")} />}
			onClick={() => void play(appId)}
		>
			Play
		</Button>
	);
}
