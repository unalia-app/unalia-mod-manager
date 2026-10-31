"use client";

import { Button, DiscordIcon } from "@modalia/ui";
import { useEffect } from "react";

import { betaBubbleOnScreen } from "@/components/feedback/beta-bubble";
import { Bubble, bubbleDismissed, useBubble } from "@/components/feedback/bubble";
import { MOD_INSTALLED } from "@/lib/installs";
import { DISCORD_URL } from "@/lib/links";
import { openExternal } from "@/lib/open";

const DISMISSED = "modalia.discordInviteDismissed";
const SHOW_AFTER_MS = 3_000;

/**
 * Invitation to the Discord, right after a mod installs: the app has just
 * proved itself, and the player isn't busy with anything else. Closed or
 * used, it never comes back. The beta bubble on screen: the next install.
 */
export function DiscordBubble() {
	const { visible, leaving, show, dismiss } = useBubble(DISMISSED);

	useEffect(() => {
		if (process.env.NODE_ENV !== "development") return;
		// Dev preview: Ctrl+Shift+D (`DevShortcuts`).
		window.addEventListener("unalia:preview-discord", show);
		return () => window.removeEventListener("unalia:preview-discord", show);
	}, [show]);

	useEffect(() => {
		let timer: number | undefined;
		const installed = () => {
			window.clearTimeout(timer);
			timer = window.setTimeout(() => {
				if (!bubbleDismissed(DISMISSED) && !betaBubbleOnScreen()) show();
			}, SHOW_AFTER_MS);
		};
		window.addEventListener(MOD_INSTALLED, installed);
		return () => {
			window.removeEventListener(MOD_INSTALLED, installed);
			window.clearTimeout(timer);
		};
	}, [show]);

	if (!visible) return null;

	return (
		<Bubble leaving={leaving} onDismiss={dismiss}>
			<p className="pr-5 text-sm leading-snug font-semibold text-fg">Unalia has a Discord</p>
			<p className="mt-1 text-xs leading-relaxed text-fg-subtle">
				Get help with a mod, suggest what comes next, and meet other players.
			</p>
			<Button
				className="mt-3 w-full"
				leftIcon={<DiscordIcon className="size-3.5" />}
				onClick={() => {
					void openExternal(DISCORD_URL);
					dismiss();
				}}
			>
				Join the Discord
			</Button>
		</Bubble>
	);
}
