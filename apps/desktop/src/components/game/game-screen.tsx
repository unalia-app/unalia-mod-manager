"use client";

import { Badge, Button, Segmented, Skeleton } from "@modalia/ui";
import { ArrowLeft, Gamepad2 } from "lucide-react";
import { useRouter, useSearchParams } from "next/navigation";
import { type CSSProperties, type ReactNode, useEffect, useState } from "react";

import { Ambience } from "@/components/ambience";
import { AppHeader } from "@/components/app-header";
import { GameCover } from "@/components/game-cover";
import { Discover } from "@/components/game/discover";
import { GameSwitcher } from "@/components/game/game-switcher";
import { MyMods } from "@/components/game/my-mods";
import { PendingBanner } from "@/components/game/pending-banner";
import { PlayButton } from "@/components/game/play-button";
import { Notice } from "@/components/notice";
import { useAccent } from "@/hooks/use-accent";
import { type SteamGame, gameSubtitle } from "@/lib/engine";
import { findGame, rememberGame } from "@/lib/games-store";
import { useInstalled } from "@/lib/installs";

type Tab = "discover" | "mine";


/** Écran d'un jeu : ambiance du jeu, onglets Découvrir et Mes mods (SPEC §3.2). */
export function GameScreen() {
	const appId = Number(useSearchParams().get("id"));
	// `undefined` : chargement ; `null` : jeu introuvable.
	const [game, setGame] = useState<SteamGame | null | undefined>(undefined);
	const [tab, setTab] = useState<Tab>("discover");
	const accent = useAccent(game?.appId ?? null);
	const installedCount = useInstalled(appId)?.length ?? 0;
	const tabs: { value: Tab; label: ReactNode }[] = [
		{ value: "discover", label: "Discover" },
		{
			value: "mine",
			label: (
				<span className="flex items-center gap-1.5">
					My mods
					{installedCount > 0 && (
						<span className="rounded-full bg-surface-strong px-1.5 font-tight text-[10px] text-fg-muted">
							{installedCount}
						</span>
					)}
				</span>
			),
		},
	];

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
							<div className="min-w-0 flex-1 animate-fade-up pb-1 [animation-delay:60ms]">
								<Badge tone="accent">{gameSubtitle(game)}</Badge>
								{/* `truncate` coupe ce qui dépasse : le pb-1 laisse la place aux jambages (g, j, p). */}
								<h1 className="mt-2 truncate pb-1 text-4xl leading-tight font-semibold tracking-tight">{game.name}</h1>
								<p className="mt-1.5 text-sm text-fg-subtle">
									Community mods from Thunderstore. Modalia installs the loader and dependencies for you.
								</p>
							</div>
							<div className="shrink-0 animate-fade-up pb-1 [animation-delay:90ms]">
								<PlayButton appId={game.appId} />
							</div>
						</section>

						<PendingBanner appId={game.appId} />

						<div className="mt-10 animate-fade-up [animation-delay:120ms]">
							<Segmented label="Game sections" variant="tabs" items={tabs} value={tab} onChange={setTab} />
						</div>

						<div className="mt-6 animate-fade-up [animation-delay:160ms]">
							{tab === "discover" ? (
								<Discover community={game.profile.community} appId={game.appId} />
							) : (
								<MyMods appId={game.appId} onDiscover={() => setTab("discover")} />
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
			Pick another game
		</Button>
	);
}

function GameMissing() {
	return (
		<div className="glass mt-16 max-w-xl">
			<Notice icon={<Gamepad2 className="size-5 text-fg-muted" />} title="This game isn't installed anymore" action={<BackHome />}>
				Steam can't find it on this computer. If it's on an unplugged drive, plug it back in and come back here.
			</Notice>
		</div>
	);
}

function NotModdable({ game }: { game: SteamGame }) {
	return (
		<div className="glass mt-16 max-w-xl">
			<Notice icon={<Gamepad2 className="size-5 text-fg-muted" />} title={`${game.name} isn't supported yet`} action={<BackHome />}>
				Modalia supports BepInEx, MelonLoader and Shimloader games for now. More are coming soon.
			</Notice>
		</div>
	);
}
