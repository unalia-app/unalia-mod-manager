"use client";

import { cn } from "@modalia/ui";
import { useState } from "react";

import { type SteamGame, artworkUrl } from "@/lib/engine";
import { initials } from "@/lib/text";

/** Jaquette du cache de Steam ; initiales tant qu'elle charge ou si elle manque. */
export function GameCover({
	game,
	dimmed = false,
	className,
}: {
	game: SteamGame;
	dimmed?: boolean;
	/** Taille : `h-12 w-8` par défaut, toujours au format 2:3. */
	className?: string;
}) {
	const [status, setStatus] = useState<"loading" | "loaded" | "missing">("loading");

	return (
		<span
			aria-hidden
			className={cn(
				"relative flex h-12 w-8 shrink-0 items-center justify-center overflow-hidden rounded-md border border-line bg-gradient-to-br from-white/[0.08] to-white/[0.02] font-tight text-[11px] font-semibold text-fg-muted",
				dimmed && "opacity-50 grayscale",
				className,
			)}
		>
			{status !== "loaded" && initials(game.name)}
			{status !== "missing" && (
				<img
					src={artworkUrl(game.appId, "cover")}
					alt=""
					loading="lazy"
					decoding="async"
					onLoad={() => setStatus("loaded")}
					onError={() => setStatus("missing")}
					className={cn(
						"absolute inset-0 size-full object-cover transition-opacity duration-300",
						status === "loaded" ? "opacity-100" : "opacity-0",
					)}
				/>
			)}
		</span>
	);
}
