"use client";

import { Badge, cn } from "@modalia/ui";
import { MessageSquarePlus, Settings } from "lucide-react";
import Link from "next/link";
import { type ReactNode, type RefObject, useEffect, useState } from "react";

import { WindowControls } from "@/components/window-controls";
import { openFeedback } from "@/lib/feedback";
import { usePlatform } from "@/lib/platform";

/**
 * App header: logo (back to home) and screen-specific content. Starts
 * after the macOS traffic lights.
 *
 * It is also the title bar (no system bar): window handle, double click to
 * maximize, and on Windows Modalia's window buttons. Tauri only moves the
 * window when the header itself is grabbed, so its buttons stay
 * clickable. It sticks to the top so the handle is always there.
 */
export function AppHeader({
	brandRef,
	brandHidden = false,
	actions,
	children,
}: {
	/** Logo, destination de l'envol de l'intro. */
	brandRef?: RefObject<HTMLSpanElement | null>;
	/** Hidden until the intro has placed the logo. */
	brandHidden?: boolean;
	children?: ReactNode;
	/** Screen actions, on the right (e.g. "Play"), always visible. */
	actions?: ReactNode;
}) {
	const scrolled = useScrolled();
	const platform = usePlatform();
	const mac = platform === "mac";

	return (
		<header
			data-tauri-drag-region
			className={cn(
				"sticky top-0 z-20 flex h-15 items-center gap-5 border-b pt-1 transition-[background-color,border-color] duration-300",
				// macOS: the traffic lights take the left side.
				// Windows: the close icon (14 px centered in a 44 px button)
				// 24 px from the edge, like the logo on the left.
				mac ? "pr-6 pl-22" : "pr-[9px] pl-6",
				scrolled ? "border-line bg-bg/75 backdrop-blur-xl" : "border-transparent",
			)}
		>
			<Link href="/" className="flex shrink-0 items-center gap-2.5" aria-label="Unalia Mod Manager, home">
				{/* Same layout as the site's logo (apps/site, components/logo.tsx):
				    UNALIA in bold italic, "Mod manager" in small caps below.
				    27 px: UNALIA ends at the same spot as "Mod manager". */}
				<span className="flex flex-col items-start leading-none">
					<span ref={brandRef} className={cn("text-[27px] leading-[0.9] font-bold tracking-tight italic", brandHidden && "opacity-0")}>
						UNALIA
					</span>
					{/* Unalia is the parent brand: here, its mod manager. Tight to
					    the word, and shifted by the italic slant. */}
					<span
						className={cn(
							"mt-px pl-[3px] font-tight text-[9px] leading-none font-semibold tracking-[0.3em] text-fg-subtle uppercase transition-[opacity,translate] duration-500 ease-out",
							brandHidden ? "translate-x-1 opacity-0" : "opacity-100",
						)}
					>
						Mod manager
					</span>
				</span>
				<Badge
					className={cn(
						"transition-[opacity,translate] duration-500 ease-out",
						brandHidden ? "translate-x-1 opacity-0" : "opacity-100",
					)}
				>
					BETA
				</Badge>
			</Link>
			{children}
			{/* Centered on the UNALIA word rather than the whole logo ("Mod manager"
			    below makes it taller than the buttons). */}
			<div className="ml-auto flex -translate-y-[6px] items-center gap-2">
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
