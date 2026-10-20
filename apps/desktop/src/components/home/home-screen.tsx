"use client";

import { Badge, cn } from "@modalia/ui";
import { type CSSProperties, useEffect, useRef, useState } from "react";

import { Ambience } from "@/components/ambience";
import { Intro, introPlayed } from "@/components/intro";
import { GamePicker } from "@/components/home/game-picker";
import { type SteamGame, gameAccent } from "@/lib/engine";

export function HomeScreen() {
	// Jeu mis en avant dans la liste (survol ou clavier) : l'ambiance le suit.
	const [focused, setFocused] = useState<SteamGame | null>(null);
	const accent = useAccent(focused?.appId ?? null);

	const brand = useRef<HTMLSpanElement>(null);
	const [intro, setIntro] = useState<"playing" | "revealing" | "done">(() =>
		introPlayed() ? "done" : "playing",
	);
	const reveal = useRef(() => setIntro("revealing")).current;
	const finish = useRef(() => setIntro("done")).current;

	return (
		<main
			className={cn("accent-transition relative min-h-dvh overflow-hidden", intro === "playing" && "intro-pending")}
			style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
		>
			{intro !== "done" && <Intro target={brand} onReveal={reveal} onDone={finish} />}

			<Ambience appId={focused?.appId ?? null} />

			<header className="relative z-10 flex h-14 items-center pl-22 pr-6">
				<span className="flex items-center gap-2 text-lg font-bold tracking-tight">
					{/* Destination de l'envol de l'intro : caché tant qu'il n'est pas arrivé. */}
					<span ref={brand} className={cn("text-xl italic", intro !== "done" && "opacity-0")}>
						MODALIA
					</span>
					<Badge
						className={cn(
							"transition-[opacity,transform] duration-500 ease-out",
							intro === "done" ? "opacity-100" : "translate-x-1 opacity-0",
						)}
					>
						ALPHA
					</Badge>
				</span>
			</header>

			<section className="relative z-10 mx-auto w-full max-w-[38rem] px-6 pt-[14vh] pb-16">
				<h1 className="animate-fade-up text-5xl font-semibold leading-[1.1] tracking-tight">
					Quel jeu veux-tu
					<br />
					<span className="text-fg-muted">modder ?</span>
				</h1>
				<p className="mt-4 animate-fade-up text-base text-fg-subtle [animation-delay:60ms]">
					Choisis tes mods, Modalia s'occupe du reste.
				</p>

				<div className="mt-9 animate-fade-up [animation-delay:120ms]">
					<GamePicker onFocusChange={setFocused} />
				</div>
			</section>
		</main>
	);
}

function useAccent(appId: number | null): string | null {
	const [accent, setAccent] = useState<string | null>(null);

	useEffect(() => {
		if (appId === null) {
			setAccent(null);
			return;
		}
		let current = true;
		gameAccent(appId).then((value) => current && setAccent(value));
		return () => {
			current = false;
		};
	}, [appId]);

	return accent;
}
