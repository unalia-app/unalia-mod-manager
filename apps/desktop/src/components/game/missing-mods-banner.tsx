"use client";

import { Button } from "@modalia/ui";
import { RotateCcw, TriangleAlert, X } from "lucide-react";
import { useEffect, useState } from "react";

import { toast } from "@/components/toaster";
import { type SteamGame, missingMods, putBackMods, splitError } from "@/lib/engine";
import { refreshInstalled, useInstalled } from "@/lib/installs";

/**
 * Des mods actifs ont quitté le dossier du jeu : Steam l'a réinstallé ou a
 * vérifié ses fichiers. Le stockage les a encore, on les repose en un clic.
 */
export function MissingModsBanner({ game }: { game: SteamGame }) {
	const installed = useInstalled(game.appId);
	const [missing, setMissing] = useState<string[]>([]);
	const [hidden, setHidden] = useState(false);
	const [busy, setBusy] = useState(false);

	useEffect(() => {
		if (!installed) return;
		let current = true;
		const check = () =>
			missingMods(game.appId).then(
				(names) => current && setMissing(names),
				() => {},
			);
		void check();
		// Le jeu a pu être réinstallé pendant que l'appli était ouverte.
		window.addEventListener("focus", check);
		return () => {
			current = false;
			window.removeEventListener("focus", check);
		};
	}, [game.appId, installed]);

	if (hidden || missing.length === 0) return null;

	const putBack = async () => {
		setBusy(true);
		try {
			const count = await putBackMods(game.appId);
			await refreshInstalled(game.appId);
			toast({ title: `${count} mod${count > 1 ? "s" : ""} put back` });
		} catch (error) {
			const { message, details } = splitError(error);
			toast({ title: message, details: details ?? undefined, tone: "error" });
		} finally {
			setBusy(false);
		}
	};

	const count = missing.length;
	const names = count <= 3 ? missing.join(", ") : `${missing.slice(0, 2).join(", ")} and ${count - 2} more`;

	return (
		<div className="mt-6 flex animate-row-in flex-wrap items-center gap-x-4 gap-y-3 rounded-xl border border-warning/25 bg-warning/[0.07] px-4 py-3">
			<span className="flex min-w-0 flex-1 items-start gap-2 text-sm text-fg">
				<TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" />
				<span>
					<span className="font-medium">
						{count} mod{count > 1 ? "s are" : " is"} missing from the game folder.
					</span>{" "}
					<span className="text-fg-muted">
						{names}. This happens when Steam reinstalls {game.name} or verifies its files. Unalia still has{" "}
						{count > 1 ? "them" : "it"}: nothing to download.
					</span>
				</span>
			</span>
			<div className="flex items-center gap-2">
				<Button
					size="sm"
					variant="outline"
					leftIcon={<RotateCcw className="size-3.5" />}
					disabled={busy}
					onClick={() => void putBack()}
				>
					Put {count > 1 ? "them" : "it"} back
				</Button>
				<button
					type="button"
					aria-label="Hide this message"
					onClick={() => setHidden(true)}
					className="cursor-pointer rounded-md p-1.5 text-fg-faint transition-colors hover:bg-white/10 hover:text-fg"
				>
					<X className="size-3.5" />
				</button>
			</div>
		</div>
	);
}
