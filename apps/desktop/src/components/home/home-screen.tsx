"use client";

import { cn } from "@modalia/ui";
import { type CSSProperties, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Ambience } from "@/components/ambience";
import { AppHeader } from "@/components/app-header";
import { ContinueCard } from "@/components/home/continue-card";
import { GameLibrary, type LibraryState } from "@/components/home/game-library";
import { Intro, introPlayed } from "@/components/intro";
import { useAccent } from "@/hooks/use-accent";
import { type SteamGame, isModdable } from "@/lib/engine";
import { loadGames, recentGameIds, withRecentsFirst } from "@/lib/games-store";

const REFRESH_EVERY_MS = 5_000;

/**
 * Dashboard home: the last modded game large, then the library. At the
 * very first launch (no recent game), Unalia's question replaces the
 * card.
 */
export function HomeScreen() {
	const [state, setState] = useState<LibraryState>({ status: "loading" });
	// Game hovered in the grid: the ambience follows it.
	const [focused, setFocused] = useState<SteamGame | null>(null);
	const accent = useAccent(focused?.appId ?? null);

	const brand = useRef<HTMLSpanElement>(null);
	const [intro, setIntro] = useState<"playing" | "revealing" | "done">(() =>
		introPlayed() ? "done" : "playing",
	);
	const reveal = useRef(() => setIntro("revealing")).current;
	const finish = useRef(() => setIntro("done")).current;

	const scan = useCallback((fresh = false) => {
		setState({ status: "loading" });
		loadGames({ fresh }).then(
			(result) => setState({ status: "ready", scan: result }),
			(error: unknown) => setState({ status: "error", message: String(error) }),
		);
	}, []);

	useEffect(() => scan(), [scan]);

	// New scan without emptying the grid: a game installed by Steam while the
	// app runs shows up without restarting it. A failure keeps the list on
	// screen.
	const [refreshing, setRefreshing] = useState(false);
	const lastRefresh = useRef(0);
	const refresh = useCallback(() => {
		lastRefresh.current = Date.now();
		setRefreshing(true);
		loadGames({ fresh: true })
			.then(
				(result) => setState({ status: "ready", scan: result }),
				() => {},
			)
			.finally(() => setRefreshing(false));
	}, []);

	// Back to the window (from Steam, often): read the library again, at
	// most once every 5 seconds.
	useEffect(() => {
		const onFocus = () => {
			const due = Date.now() - lastRefresh.current > REFRESH_EVERY_MS;
			if (state.status === "ready" && !refreshing && due) refresh();
		};
		window.addEventListener("focus", onFocus);
		return () => window.removeEventListener("focus", onFocus);
	}, [state.status, refreshing, refresh]);

	const { featured, games, others } = useMemo(() => {
		const all = state.status === "ready" ? state.scan.games : [];
		const moddable = withRecentsFirst(all.filter(isModdable));
		// The most recent one still installed: a game uninstalled or on an
		// unplugged drive doesn't make the card disappear.
		const featured =
			recentGameIds()
				.map((id) => moddable.find((g) => g.appId === id))
				.find((g) => g !== undefined) ?? null;
		return { featured, games: moddable, others: all.filter((g) => !isModdable(g)) };
	}, [state]);

	return (
		<main
			className={cn("accent-transition relative min-h-dvh overflow-clip", intro === "playing" && "intro-pending")}
			style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
		>
			{intro !== "done" && <Intro target={brand} onReveal={reveal} onDone={finish} />}

			<Ambience appId={focused?.appId ?? null} />

			<AppHeader brandRef={brand} brandHidden={intro !== "done"} />

			<div className="relative z-10 mx-auto w-full max-w-[120rem] px-8 pt-8 pb-20 xl:px-12">
				{featured ? (
					<ContinueCard game={featured} />
				) : (
					state.status !== "loading" && (
						<header className="pt-6">
							<h1 className="animate-fade-up text-5xl font-semibold leading-[1.1] tracking-tight">
								Which game do you
								<br />
								<span className="text-fg-muted">want to mod?</span>
							</h1>
							<p className="mt-4 animate-fade-up text-base text-fg-subtle [animation-delay:60ms]">
								Pick your mods, Unalia handles the rest.
							</p>
						</header>
					)
				)}

				<div className="my-8 h-px bg-line" />

				<GameLibrary
					state={state}
					games={games}
					others={others}
					onRetry={() => scan(true)}
					onRefresh={refresh}
					refreshing={refreshing}
					onFocusChange={setFocused}
				/>
			</div>
		</main>
	);
}
