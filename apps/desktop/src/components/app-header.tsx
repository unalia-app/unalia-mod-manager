"use client";

import { Badge, cn } from "@modalia/ui";
import Link from "next/link";
import { type ReactNode, type RefObject, useEffect, useState } from "react";

/**
 * En-tête de l'appli : logo (retour à l'accueil) et contenu propre à
 * l'écran. Commence après les feux tricolores de macOS.
 *
 * C'est aussi la poignée de la fenêtre (pas de barre de titre système) :
 * Tauri ne déplace la fenêtre que si on attrape l'en-tête lui-même, ses
 * boutons restent cliquables. Il reste collé en haut pour que la poignée
 * soit toujours là.
 */
export function AppHeader({
	brandRef,
	brandHidden = false,
	children,
}: {
	/** Logo, destination de l'envol de l'intro. */
	brandRef?: RefObject<HTMLSpanElement | null>;
	/** Caché tant que l'intro n'a pas posé le logo. */
	brandHidden?: boolean;
	children?: ReactNode;
}) {
	const scrolled = useScrolled();

	return (
		<header
			data-tauri-drag-region
			className={cn(
				"sticky top-0 z-20 flex h-14 items-center gap-5 border-b pr-6 pl-22 transition-[background-color,border-color] duration-300",
				scrolled ? "border-line bg-bg/75 backdrop-blur-xl" : "border-transparent",
			)}
		>
			<Link href="/" className="flex shrink-0 items-center gap-2 text-lg font-bold tracking-tight" aria-label="Home">
				<span ref={brandRef} className={cn("text-xl italic", brandHidden && "opacity-0")}>
					MODALIA
				</span>
				<Badge
					className={cn(
						"transition-[opacity,translate] duration-500 ease-out",
						brandHidden ? "translate-x-1 opacity-0" : "opacity-100",
					)}
				>
					ALPHA
				</Badge>
			</Link>
			{children}
		</header>
	);
}

function useScrolled(): boolean {
	const [scrolled, setScrolled] = useState(false);
	useEffect(() => {
		const update = () => setScrolled(window.scrollY > 4);
		update();
		window.addEventListener("scroll", update, { passive: true });
		return () => window.removeEventListener("scroll", update);
	}, []);
	return scrolled;
}
