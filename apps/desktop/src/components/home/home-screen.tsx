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

/**
 * Accueil en tableau de bord : le dernier jeu moddé en grand, puis la
 * bibliothèque. Au tout premier lancement (aucun jeu récent), la question
 * d'Unalia remplace la carte.
 */
export function HomeScreen() {
	const [state, setState] = useState<LibraryState>({ status: "loading" });
	// Jeu survolé dans la grille : l'ambiance le suit.
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

	const { featured, games, others } = useMemo(() => {
		const all = state.status === "ready" ? state.scan.games : [];
		const moddable = withRecentsFirst(all.filter(isModdable));
		const last = recentGameIds()[0];
		const featured = moddable.find((g) => g.appId === last) ?? null;
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
					onFocusChange={setFocused}
				/>
			</div>
		</main>
	);
}
