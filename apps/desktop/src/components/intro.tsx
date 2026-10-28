"use client";

import { type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";

const WORD = "UNALIA";

/** Key moments, in ms since opening. */
const T = {
	letters: 80,
	letterStagger: 45,
	fly: 1050,
	flyDuration: 620,
};

/** The intro only plays once per app launch. */
let played = false;

export function introPlayed(): boolean {
	return played;
}

/**
 * Launch animation:
 * 1. the letters of UNALIA appear one by one: fade, slight rise, from blur
 *    to sharp;
 * 2. an iridescent sheen crosses them, a glow opens, a line draws itself;
 * 3. the logo flies to its place in the header while the home screen
 *    appears below.
 *
 * `transform` and `opacity`, plus the blur of the letters and the sheen,
 * limited to the word (small surface).
 * A click or a key jumps straight to the flight. If the system asks for
 * reduced motion, a simple fade.
 */
export function Intro({
	target,
	onReveal,
	onDone,
}: {
	/** Header logo, destination of the flight. */
	target: RefObject<HTMLElement | null>;
	/** Start of the flight: the home screen can start its entrances. */
	onReveal: () => void;
	/** End: the intro can be removed. */
	onDone: () => void;
}) {
	const word = useRef<HTMLSpanElement>(null);
	const backdrop = useRef<HTMLDivElement>(null);
	const decor = useRef<HTMLDivElement>(null);
	const [reduced, setReduced] = useState(false);
	const flying = useRef(false);

	useLayoutEffect(() => {
		played = true;
		setReduced(window.matchMedia("(prefers-reduced-motion: reduce)").matches);
	}, []);

	useEffect(() => {
		const callbacks = { onReveal, onDone };

		const fly = () => {
			if (flying.current) return;
			flying.current = true;
			word.current?.setAttribute("data-flying", "");
			callbacks.onReveal();

			const from = word.current?.getBoundingClientRect();
			const to = target.current?.getBoundingClientRect();
			const ease = "cubic-bezier(0.32, 0.72, 0, 1)";
			const finish = () => callbacks.onDone();

			if (!from || !to || reduced) {
				backdrop.current?.parentElement
					?.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 250, fill: "forwards" })
					.finished.then(finish, finish);
				return;
			}

			const scale = to.width / from.width;
			const dx = to.left + to.width / 2 - (from.left + from.width / 2);
			const dy = to.top + to.height / 2 - (from.top + from.height / 2);

			word.current
				?.animate([{ transform: "none" }, { transform: `translate(${dx}px, ${dy}px) scale(${scale})` }], {
					duration: T.flyDuration,
					easing: ease,
					fill: "forwards",
				})
				.finished.then(finish, finish);
			backdrop.current?.animate([{ opacity: 1 }, { opacity: 0 }], {
				duration: T.flyDuration - 120,
				delay: 80,
				easing: "ease-out",
				fill: "forwards",
			});
			decor.current?.animate([{ opacity: 1 }, { opacity: 0 }], {
				duration: 220,
				easing: "ease-out",
				fill: "forwards",
			});
		};

		const timer = window.setTimeout(fly, reduced ? 350 : T.fly);
		window.addEventListener("pointerdown", fly, { once: true });
		window.addEventListener("keydown", fly, { once: true });
		return () => {
			window.clearTimeout(timer);
			window.removeEventListener("pointerdown", fly);
			window.removeEventListener("keydown", fly);
		};
	}, [reduced, target, onReveal, onDone]);

	return (
		<div aria-hidden className="pointer-events-none fixed inset-0 z-[60]">
			<div ref={backdrop} className="absolute inset-0 bg-bg" />

			<div ref={decor} className="absolute inset-0">
				{/* Glow that opens behind the logo (centered by its keyframe: no
				    translate class, it would add to the transform) */}
				<div className="intro-bloom absolute top-1/2 left-1/2 size-[70vmin] rounded-full bg-[radial-gradient(closest-side,rgb(255_255_255/0.09),transparent)]" />
				{/* The product, small under the brand */}
				<p
					className="absolute top-[calc(50%+4.4rem)] left-0 w-full text-center font-tight text-xs font-medium tracking-[0.35em] text-fg-subtle uppercase"
					style={{ animation: "fade-up 500ms var(--ease-out) 520ms both" }}
				>
					Mod manager
				</p>
				{/* Line that draws itself under the logo */}
				<div className="absolute top-[calc(50%+3.4rem)] left-1/2 h-px w-56 -translate-x-1/2 overflow-hidden">
					<div className="intro-line h-full w-full bg-gradient-to-r from-transparent via-white/60 to-transparent" />
				</div>
			</div>

			<div className="absolute inset-0 flex items-center justify-center">
				<span
					ref={word}
					className="intro-word relative inline-block origin-center text-[88px] leading-none font-bold tracking-tight italic"
				>
					<span className="block">
						{WORD.split("").map((letter, i) => (
							<span
								key={`${letter}-${i}`}
								className="intro-letter inline-block"
								style={{ animationDelay: `${T.letters + i * T.letterStagger}ms` }}
							>
								{letter}
							</span>
						))}
					</span>
					{/* Iridescent sheen, clipped to the shape of the letters */}
					<span className="absolute inset-0 block">
						{WORD.split("").map((letter, i) => (
							<span key={`${letter}-${i}`} className="intro-sheen inline-block bg-clip-text text-transparent">
								{letter}
							</span>
						))}
					</span>
				</span>
			</div>
		</div>
	);
}
