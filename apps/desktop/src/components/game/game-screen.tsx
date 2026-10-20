"use client";

import { Badge, Button, Segmented, Skeleton } from "@modalia/ui";
import { ArrowLeft, Gamepad2 } from "lucide-react";
import { useRouter, useSearchParams } from "next/navigation";
import { type CSSProperties, useEffect, useState } from "react";

import { Ambience } from "@/components/ambience";
import { AppHeader } from "@/components/app-header";
import { GameCover } from "@/components/game-cover";
import { Discover } from "@/components/game/discover";
import { GameSwitcher } from "@/components/game/game-switcher";
import { MyMods } from "@/components/game/my-mods";
import { Notice } from "@/components/notice";
import { useAccent } from "@/hooks/use-accent";
import { type SteamGame, gameSubtitle } from "@/lib/engine";
import { findGame, rememberGame } from "@/lib/games-store";

type Tab = "discover" | "mine";

const TABS: { value: Tab; label: string }[] = [
	{ value: "discover", label: "Découvrir" },
	{ value: "mine", label: "Mes mods" },
];

/** Écran d'un jeu : ambiance du jeu, onglets Découvrir et Mes mods (SPEC §3.2). */
export function GameScreen() {
	const appId = Number(useSearchParams().get("id"));
	// `undefined` : chargement ; `null` : jeu introuvable.
	const [game, setGame] = useState<SteamGame | null | undefined>(undefined);
	const [tab, setTab] = useState<Tab>("discover");
	const accent = useAccent(game?.appId ?? null);

	useEffect(() => {
		setGame(undefined);
		setTab("discover");
		findGame(appId).then(
			(found) => {
				setGame(found);
				if (found) rememberGame(found.appId);
			},
			() => setGame(null),
		);
	}, [appId]);

	return (
		<main
			className="accent-transition relative min-h-dvh overflow-x-clip"
			style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
		>
			<Ambience appId={game?.appId ?? null} />

			<AppHeader>{game && <GameSwitcher current={game} />}</AppHeader>

			<div className="relative z-10 mx-auto w-full max-w-6xl px-8 pb-20">
				{game === undefined && <HeroSkeleton />}
				{game === null && <GameMissing />}
				{game && !game.moddable && <NotModdable game={game} />}

				{game?.moddable && game.profile && (
					<>
						<section key={game.appId} className="flex items-end gap-6 pt-10">
							<GameCover
								game={game}
								className="h-36 w-24 animate-fade-up rounded-lg border-line-strong text-lg shadow-2xl shadow-black/50"
							/>
							<div className="min-w-0 animate-fade-up pb-1 [animation-delay:60ms]">
								<Badge tone="accent">{gameSubtitle(game)}</Badge>
								{/* `truncate` coupe ce qui dépasse : le pb-1 laisse la place aux jambages (g, j, p). */}
								<h1 className="mt-2 truncate pb-1 text-4xl leading-tight font-semibold tracking-tight">{game.name}</h1>
								<p className="mt-1.5 text-sm text-fg-subtle">
									Mods de la communauté, depuis Thunderstore. Modalia installe le loader et les dépendances pour toi.
								</p>
							</div>
						</section>

						<div className="mt-10 animate-fade-up [animation-delay:120ms]">
							<Segmented label="Sections du jeu" variant="tabs" items={TABS} value={tab} onChange={setTab} />
						</div>

						<div className="mt-6 animate-fade-up [animation-delay:160ms]">
							{tab === "discover" ? (
								<Discover community={game.profile.community} />
							) : (
								<MyMods onDiscover={() => setTab("discover")} />
							)}
						</div>
					</>
				)}
			</div>
		</main>
	);
}

function HeroSkeleton() {
	return (
		<div className="flex items-end gap-6 pt-10">
			<Skeleton className="h-36 w-24 rounded-lg" />
			<div className="flex-1 space-y-3 pb-1">
				<Skeleton className="h-5 w-28 rounded-full" />
				<Skeleton className="h-9 w-2/5" />
				<Skeleton className="h-3.5 w-3/5" />
			</div>
		</div>
	);
}

function BackHome() {
	const router = useRouter();
	return (
		<Button variant="outline" size="sm" leftIcon={<ArrowLeft className="size-3.5" />} onClick={() => router.push("/")}>
			Choisir un autre jeu
		</Button>
	);
}

function GameMissing() {
	return (
		<div className="glass mt-16 max-w-xl">
			<Notice icon={<Gamepad2 className="size-5 text-fg-muted" />} title="Ce jeu n'est plus installé" action={<BackHome />}>
				Steam ne le trouve plus sur cet ordinateur. S'il est sur un disque débranché, rebranche-le puis reviens ici.
			</Notice>
		</div>
	);
}

function NotModdable({ game }: { game: SteamGame }) {
	return (
		<div className="glass mt-16 max-w-xl">
			<Notice icon={<Gamepad2 className="size-5 text-fg-muted" />} title={`${game.name} n'est pas encore pris en charge`} action={<BackHome />}>
				Modalia commence par les jeux qui utilisent BepInEx. D'autres arrivent bientôt.
			</Notice>
		</div>
	);
}
