"use client";

import { Badge, Button, Segmented, Skeleton, cn } from "@modalia/ui";
import { ArrowLeft, Gamepad2 } from "lucide-react";
import { useRouter, useSearchParams } from "next/navigation";
import { type CSSProperties, type ReactNode, type RefObject, useEffect, useRef, useState } from "react";

import { Ambience } from "@/components/ambience";
import { AppHeader } from "@/components/app-header";
import { GameCover } from "@/components/game-cover";
import { AntiCheatBanner } from "@/components/game/anti-cheat-banner";
import { Discover, useOnHexium } from "@/components/game/discover";
import { GameUpdatedBanner } from "@/components/game/game-updated-banner";
import { GameVersion } from "@/components/game/game-version";
import { MissingModsBanner } from "@/components/game/missing-mods-banner";
import { GameSwitcher } from "@/components/game/game-switcher";
import { MyMods } from "@/components/game/my-mods";
import { NexusConnect } from "@/components/game/nexus-connect";
import { PendingBanner } from "@/components/game/pending-banner";
import { PlayButton } from "@/components/game/play-button";
import { RemoveGame } from "@/components/game/remove-game";
import { ResetGame } from "@/components/game/reset-game";
import { SteamLaunch } from "@/components/game/steam-launch";
import { Notice } from "@/components/notice";
import { useAccent } from "@/hooks/use-accent";
import { type SteamGame, gameSubtitle } from "@/lib/engine";
import { findGame, rememberGame } from "@/lib/games-store";
import { useInstalled } from "@/lib/installs";

type Tab = "discover" | "mine";


/** A game's screen: game ambience, Discover and My mods tabs. */
export function GameScreen() {
	const appId = Number(useSearchParams().get("id"));
	// `undefined`: loading; `null`: game not found.
	const [game, setGame] = useState<SteamGame | null | undefined>(undefined);
	const [tab, setTab] = useState<Tab>("discover");
	const accent = useAccent(game?.appId ?? null);
	const installedCount = useInstalled(appId)?.length ?? 0;
	const onHexium = useOnHexium(game?.profile?.community ?? "");
	// Header "Play": only when the banner's one has left the screen, never
	// both at once.
	const heroPlay = useRef<HTMLDivElement>(null);
	const heroPlayVisible = useVisible(heroPlay, game);
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

			<AppHeader
				actions={
					game?.moddable && (
						<div
							inert={heroPlayVisible}
							aria-hidden={heroPlayVisible}
							className={cn(
								"transition-[opacity,translate] duration-300 ease-out",
								heroPlayVisible ? "pointer-events-none -translate-y-1 opacity-0" : "opacity-100",
							)}
						>
							<PlayButton appId={game.appId} compact />
						</div>
					)
				}
			>
				{game && <GameSwitcher current={game} />}
			</AppHeader>

			<div className="relative z-10 mx-auto w-full max-w-[120rem] px-8 pb-20 xl:px-12">
				{game === undefined && <HeroSkeleton />}
				{game === null && <GameMissing />}
				{game && !game.moddable && <NotModdable game={game} />}

				{game?.moddable && game.profile && (
					<>
						{/* Above the tabs: the "Game version" menu opens toward them. */}
						<section key={game.appId} className="relative z-20 flex items-end gap-6 pt-10">
							<GameCover
								game={game}
								className="h-36 w-24 animate-fade-up rounded-lg border-line-strong text-lg shadow-2xl shadow-black/50"
							/>
							<div className="min-w-0 flex-1 animate-fade-up pb-1 [animation-delay:60ms]">
								<Badge tone="accent">{gameSubtitle(game)}</Badge>
								{/* `truncate` cuts what overflows: pb-1 leaves room for descenders (g, j, p). */}
								<h1 className="mt-2 truncate pb-1 text-4xl leading-tight font-semibold tracking-tight">{game.name}</h1>
								<p className="mt-1.5 text-sm text-fg-subtle">
									{game.profile.nexus
										? "Mods from Nexus Mods. Unalia puts every file in the right place and installs the frameworks they need, from their authors."
										: `Community mods from ${onHexium ? "Thunderstore and Hexium" : "Thunderstore"}. Unalia installs the loader and dependencies for you.`}
								</p>
							</div>
							<div ref={heroPlay} className="flex shrink-0 animate-fade-up flex-col items-end pb-1 [animation-delay:90ms]">
								<PlayButton appId={game.appId} />
								<SteamLaunch game={game} />
								<GameVersion game={game} />
								<div className="mt-3 flex items-center gap-2">
									<ResetGame game={game} />
									<RemoveGame game={game} />
								</div>
							</div>
						</section>

						<AntiCheatBanner game={game} />
						<PendingBanner appId={game.appId} />
						<MissingModsBanner game={game} />
						<GameUpdatedBanner game={game} />

						<div className="mt-10 animate-fade-up [animation-delay:120ms]">
							<Segmented label="Game sections" variant="tabs" items={tabs} value={tab} onChange={setTab} />
						</div>

						<div className="mt-6 animate-fade-up [animation-delay:160ms]">
							{tab === "discover" ? (
								<>
									{game.profile.nexus && <NexusConnect />}
									<Discover community={game.profile.community} appId={game.appId} />
								</>
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

/** Is the element on screen? (true while we don't know). */
function useVisible(ref: RefObject<HTMLElement | null>, trigger: unknown): boolean {
	const [visible, setVisible] = useState(true);
	// `trigger` restarts the observation when the element appears (game loaded).
	useEffect(() => {
		const node = ref.current;
		if (!node) return;
		// The top margin counts the sticky header (h-15): under it, the button
		// is no longer visible.
		const observer = new IntersectionObserver(([entry]) => setVisible(entry?.isIntersecting ?? true), {
			rootMargin: "-60px 0px 0px 0px",
		});
		observer.observe(node);
		return () => observer.disconnect();
	}, [ref, trigger]);
	return visible;
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
				Unalia supports BepInEx, MelonLoader and Shimloader games, Unreal Engine games on Nexus Mods,
				Cyberpunk 2077, Skyrim Special Edition and Fallout 4, for now. More are coming soon.
			</Notice>
		</div>
	);
}
