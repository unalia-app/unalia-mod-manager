"use client";

import { cn } from "@modalia/ui";
import { type CSSProperties, useRef, useState } from "react";

import { Ambience } from "@/components/ambience";
import { AppHeader } from "@/components/app-header";
import { Intro, introPlayed } from "@/components/intro";
import { GamePicker } from "@/components/home/game-picker";
import { useAccent } from "@/hooks/use-accent";
import type { SteamGame } from "@/lib/engine";

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
			className={cn("accent-transition relative min-h-dvh overflow-clip", intro === "playing" && "intro-pending")}
			style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
		>
			{intro !== "done" && <Intro target={brand} onReveal={reveal} onDone={finish} />}

			<Ambience appId={focused?.appId ?? null} />

			<AppHeader brandRef={brand} brandHidden={intro !== "done"} />

			<section className="relative z-10 mx-auto w-full max-w-[38rem] px-6 pt-[14vh] pb-16">
				<h1 className="animate-fade-up text-5xl font-semibold leading-[1.1] tracking-tight">
					Which game do you
					<br />
					<span className="text-fg-muted">want to mod?</span>
				</h1>
				<p className="mt-4 animate-fade-up text-base text-fg-subtle [animation-delay:60ms]">
					Pick your mods, Modalia handles the rest.
				</p>

				<div className="mt-9 animate-fade-up [animation-delay:120ms]">
					<GamePicker onFocusChange={setFocused} />
				</div>
			</section>
		</main>
	);
}
