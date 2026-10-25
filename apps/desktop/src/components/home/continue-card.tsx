"use client";

import { Badge, Button, cn } from "@modalia/ui";
import { ArrowRight, Sparkles } from "lucide-react";
import { useRouter } from "next/navigation";
import { type CSSProperties, useState } from "react";

import { PlayButton } from "@/components/game/play-button";
import { useAccent } from "@/hooks/use-accent";
import { type SteamGame, artworkUrl, gameSubtitle } from "@/lib/engine";
import { gameHref, rememberGame } from "@/lib/games-store";
import { useInstalled, useUpdates } from "@/lib/installs";

/**
 * Dernier jeu moddé, en grand : sa bannière, ses mods, et de quoi jouer ou
 * reprendre sans repasser par la liste.
 */
export function ContinueCard({ game }: { game: SteamGame }) {
	const accent = useAccent(game.appId);
	const installed = useInstalled(game.appId);
	const updates = useUpdates(game.appId);
	const [hero, setHero] = useState<"loading" | "loaded" | "missing">("loading");
	const [logo, setLogo] = useState<"loading" | "loaded" | "missing">("loading");

	const mods = installed?.length;
	const router = useRouter();

	return (
		<section
			aria-label={`Continue with ${game.name}`}
			className="accent-transition relative isolate flex min-h-72 animate-fade-up items-end overflow-clip rounded-xl border border-line bg-bg"
			style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
		>
			{hero !== "missing" && (
				<img
					src={artworkUrl(game.appId, "hero")}
					alt=""
					onLoad={() => setHero("loaded")}
					onError={() => setHero("missing")}
					className={cn(
						// Fondu vers la gauche par un masque plutôt qu'un calque sombre
						// par-dessus : deux calques découpés sur l'arrondi laissaient
						// un liseré de l'image dans les coins.
						"absolute inset-0 -z-10 size-full object-cover transition-opacity duration-700 [mask-image:linear-gradient(to_right,transparent,rgb(0_0_0/0.2)_50%,rgb(0_0_0/0.9))]",
						hero === "loaded" ? "opacity-100" : "opacity-0",
					)}
				/>
			)}
			<div
				aria-hidden
				className="absolute inset-0 -z-10 bg-[radial-gradient(60%_90%_at_0%_100%,color-mix(in_oklch,var(--color-accent)_22%,transparent),transparent)]"
			/>

			<p className="absolute top-7 left-7 flex items-center gap-2 text-xs font-medium tracking-wide text-fg-subtle uppercase">
				<Sparkles className="size-3.5 text-accent" />
				Continue modding
			</p>

			<div className="flex w-full flex-wrap items-end justify-between gap-6 p-7">
				<div className="min-w-0">
					{logo !== "missing" && (
						<img
							src={artworkUrl(game.appId, "logo")}
							alt={game.name}
							onLoad={() => setLogo("loaded")}
							onError={() => setLogo("missing")}
							className={cn(
								"max-h-24 max-w-[26rem] object-contain object-left drop-shadow-[0_4px_18px_rgb(0_0_0/0.6)]",
								logo === "loaded" ? "block" : "hidden",
							)}
						/>
					)}
					{logo !== "loaded" && (
						<h2 className="truncate pb-1 text-4xl font-semibold tracking-tight text-fg">{game.name}</h2>
					)}
					<div className="mt-4 flex flex-wrap items-center gap-2">
						<Badge>{gameSubtitle(game)}</Badge>
						{mods ? <Badge>{mods === 1 ? "1 mod" : `${mods} mods`}</Badge> : null}
						{updates && updates.length > 0 && (
							<Badge tone="accent">
								{updates.length === 1 ? "1 update" : `${updates.length} updates`}
							</Badge>
						)}
					</div>
				</div>

				<div className="flex items-center gap-3">
					<Button
						variant="outline"
						size="lg"
						rightIcon={<ArrowRight className="size-4" />}
						onClick={() => {
							rememberGame(game.appId);
							router.push(gameHref(game.appId));
						}}
					>
						Manage mods
					</Button>
					<PlayButton appId={game.appId} />
				</div>
			</div>
		</section>
	);
}
