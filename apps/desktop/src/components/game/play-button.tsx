"use client";

import { Button, Spinner } from "@modalia/ui";
import { Play } from "lucide-react";

import { play, useGameStatus } from "@/lib/game-status";

/** « Play » (SPEC §3.6) : lance le jeu avec ses mods actifs. */
export function PlayButton({ appId }: { appId: number }) {
	const status = useGameStatus(appId);

	if (status?.running) {
		return (
			<div className="flex h-10 items-center gap-2.5 rounded-md border border-accent/30 bg-accent/10 px-4 text-sm font-medium text-accent">
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
			<Button size="lg" variant="outline" disabled className="disabled:opacity-100">
				<span className="flex items-center gap-2">
					<Spinner className="size-3.5" />
					Starting…
				</span>
			</Button>
		);
	}
	return (
		<Button size="lg" leftIcon={<Play className="size-4 fill-current" />} onClick={() => void play(appId)}>
			Play
		</Button>
	);
}
