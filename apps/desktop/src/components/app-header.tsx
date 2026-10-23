"use client";

import { Badge, cn } from "@modalia/ui";
import { MessageSquarePlus, Settings } from "lucide-react";
import Link from "next/link";
import { type ReactNode, type RefObject, useEffect, useState } from "react";

import { WindowControls } from "@/components/window-controls";
import { openFeedback } from "@/lib/feedback";
import { usePlatform } from "@/lib/platform";

/**
 * En-tête de l'appli : logo (retour à l'accueil) et contenu propre à
 * l'écran. Commence après les feux tricolores de macOS.
 *
 * C'est aussi la barre de titre (pas de barre système) : poignée de la
 * fenêtre, double-clic pour agrandir, et sous Windows les boutons de
 * fenêtre de Modalia. Tauri ne déplace la fenêtre que si on attrape
 * l'en-tête lui-même, ses
 * boutons restent cliquables. Il reste collé en haut pour que la poignée
 * soit toujours là.
 */
export function AppHeader({
	brandRef,
	brandHidden = false,
	actions,
	children,
}: {
	/** Logo, destination de l'envol de l'intro. */
	brandRef?: RefObject<HTMLSpanElement | null>;
	/** Caché tant que l'intro n'a pas posé le logo. */
	brandHidden?: boolean;
	children?: ReactNode;
	/** Actions de l'écran, à droite (ex. « Play »), toujours visibles. */
	actions?: ReactNode;
}) {
	const scrolled = useScrolled();
	const platform = usePlatform();
	const mac = platform === "mac";

	return (
		<header
			data-tauri-drag-region
			className={cn(
				"sticky top-0 z-20 flex h-14 items-center gap-5 border-b transition-[background-color,border-color] duration-300",
				// macOS : les feux tricolores occupent la gauche.
				// Windows : l'icône de fermeture (14 px au centre d'un bouton de
				// 44 px) à 24 px du bord, comme le logo à gauche.
				mac ? "pr-6 pl-22" : "pr-[9px] pl-6",
				scrolled ? "border-line bg-bg/75 backdrop-blur-xl" : "border-transparent",
			)}
		>
			<Link href="/" className="flex shrink-0 items-center gap-2 text-lg font-bold tracking-tight" aria-label="Unalia Mod Manager, home">
				<span ref={brandRef} className={cn("text-xl italic", brandHidden && "opacity-0")}>
					UNALIA
				</span>
				{/* Unalia est la marque mère : ici, son gestionnaire de mods. */}
				<span
					className={cn(
						"font-tight text-[11px] font-medium tracking-wide text-fg-subtle uppercase transition-[opacity,translate] duration-500 ease-out",
						brandHidden ? "translate-x-1 opacity-0" : "opacity-100",
					)}
				>
					Mod manager
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
			<div className="ml-auto flex items-center gap-2">
				{actions}
				<button
					type="button"
					onClick={openFeedback}
					aria-label="Help & Feedback"
					title="Help & Feedback"
					className="flex size-8 cursor-pointer items-center justify-center rounded-md text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg"
				>
					<MessageSquarePlus className="size-4" />
				</button>
				<Link
					href="/settings/"
					aria-label="Settings"
					title="Settings"
					className="flex size-8 items-center justify-center rounded-md text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg"
				>
					<Settings className="size-4" />
				</Link>
				{!mac && <WindowControls />}
			</div>
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
