"use client";

import { Button, Dialog, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@modalia/ui";
import { RotateCcw, RotateCw, TriangleAlert } from "lucide-react";
import { useRouter } from "next/navigation";

import { requestReset } from "@/components/game/reset-game";
import { gameHref } from "@/lib/games-store";
import { clearError, dismissFailure, install, useInstallFailure } from "@/lib/installs";

/**
 * An install just failed: the reason and what to do, in a dialog rather
 * than small on the mod card. When the engine advises "Reset mods" (files
 * left by another tool), the button is there.
 */
export function InstallErrorDialog() {
	const failure = useInstallFailure();
	const router = useRouter();

	if (!failure) return null;
	const { appId, modId, name, message, details } = failure;
	const canReset = message.includes("Reset mods");

	const retry = () => {
		dismissFailure();
		clearError(appId, modId);
		void install(appId, modId, name);
	};

	const reset = () => {
		dismissFailure();
		clearError(appId, modId);
		// The "Reset mods" dialog is on the game page.
		// (`location` rather than `useSearchParams`, which needs a Suspense in the layout.)
		const here = window.location.pathname.startsWith("/game") && new URLSearchParams(window.location.search).get("id") === String(appId);
		if (!here) router.push(gameHref(appId));
		requestReset(appId);
	};

	return (
		<Dialog open onClose={dismissFailure}>
			<DialogHeader>
				<div className="mb-1 flex size-9 items-center justify-center rounded-full bg-danger/10 text-danger">
					<TriangleAlert className="size-4.5" />
				</div>
				<DialogTitle>{name ? `${name} didn't install` : "The mod didn't install"}</DialogTitle>
				<DialogDescription>{message}</DialogDescription>
			</DialogHeader>
			{details && (
				<details className="text-xs text-fg-faint">
					<summary className="cursor-pointer select-none hover:text-fg-subtle">Technical details</summary>
					<p className="mt-2 max-h-40 overflow-y-auto rounded-lg border border-line bg-surface px-3 py-2 font-mono break-words text-fg-subtle">
						{details}
					</p>
				</details>
			)}
			<DialogFooter>
				<Button variant="secondary" onClick={dismissFailure}>
					Close
				</Button>
				{canReset ? (
					<Button leftIcon={<RotateCcw className="size-3.5" />} onClick={reset}>
						Reset mods
					</Button>
				) : (
					<Button leftIcon={<RotateCw className="size-3.5" />} onClick={retry}>
						Try again
					</Button>
				)}
			</DialogFooter>
		</Dialog>
	);
}
