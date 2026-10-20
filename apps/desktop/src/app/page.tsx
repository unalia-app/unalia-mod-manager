import { Badge } from "@modalia/ui";

import { GamePicker } from "@/components/home/game-picker";

export default function Home() {
	return (
		<main className="relative min-h-dvh overflow-hidden">
			<Ambience />

			<header className="relative z-10 flex h-14 items-center pl-22 pr-6">
				<span className="flex items-center gap-2 text-lg font-bold tracking-tight">
					<span className="italic">MODALIA</span>
					<Badge>ALPHA</Badge>
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
					<GamePicker />
				</div>
			</section>
		</main>
	);
}

/** Halo d'ambiance teinté par l'accent du jeu actif (neutre à l'accueil). */
function Ambience() {
	return (
		<div aria-hidden className="pointer-events-none absolute inset-0">
			<div className="absolute -top-1/3 left-1/2 h-[80vh] w-[120vw] -translate-x-1/2 bg-[radial-gradient(closest-side,color-mix(in_oklch,var(--color-accent)_9%,transparent),transparent)]" />
			<div className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-bg to-transparent" />
		</div>
	);
}
