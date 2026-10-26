"use client";

import { Button, Dialog, DialogDescription, DialogFooter, DialogHeader, DialogTitle, Skeleton } from "@modalia/ui";
import { Folder, FileText, RotateCcw } from "lucide-react";
import { useState } from "react";

import { toast } from "@/components/toaster";
import { type ResetPreview, type SteamGame, resetGame, resetPreview, splitError } from "@/lib/engine";
import { bytes } from "@/lib/format";
import { forgetGameMods, recheckUpdates, refreshInstalled } from "@/lib/installs";

/**
 * Remet le jeu à zéro côté mods : ceux d'Unalia le quittent, et ce que
 * d'autres outils ont laissé (à la main, Vortex…) part à la corbeille.
 * Pour repartir propre quand on ne sait plus quoi effacer.
 */
export function ResetGame({ game }: { game: SteamGame }) {
	const [open, setOpen] = useState(false);
	// `null` : chargement.
	const [preview, setPreview] = useState<ResetPreview | null>(null);
	const [resetting, setResetting] = useState(false);

	const show = () => {
		setPreview(null);
		setOpen(true);
		resetPreview(game.appId).then(setPreview, (error: unknown) => {
			setOpen(false);
			const { message, details } = splitError(error);
			toast({ title: message, details: details ?? undefined, tone: "error" });
		});
	};

	const reset = async () => {
		setResetting(true);
		try {
			const done = await resetGame(game.appId);
			forgetGameMods(game.appId);
			await refreshInstalled(game.appId);
			recheckUpdates(game.appId);
			setOpen(false);
			if (done.left.length > 0) {
				toast({
					title: "Some files couldn't be moved",
					details: `Close the game and any program using its folder, then reset again. Still there: ${done.left.join(", ")}.`,
					tone: "error",
				});
			} else {
				toast({ title: `${game.name} is back to no mods`, details: summary(done.mods, done.foreign) });
			}
		} catch (error) {
			const { message, details } = splitError(error);
			toast({ title: message, details: details ?? undefined, tone: "error" });
		} finally {
			setResetting(false);
		}
	};

	const empty = preview !== null && preview.mods === 0 && preview.foreign.length === 0;

	return (
		<>
			<Button
				variant="outline"
				size="sm"
				leftIcon={<RotateCcw className="size-3.5" />}
				title="Take every mod out of the game folder, including ones from other tools"
				onClick={show}
			>
				Reset mods
			</Button>

			<Dialog open={open} onClose={resetting ? undefined : () => setOpen(false)} className="max-w-lg">
				<DialogHeader>
					<DialogTitle>Reset mods for {game.name}?</DialogTitle>
					<DialogDescription>
						Unalia takes out every mod it installed, and sends mod files left by other tools (added by hand, Vortex…) to
						the Recycle Bin. The game itself and your saves aren't touched.
					</DialogDescription>
				</DialogHeader>

				{preview === null ? (
					<div className="space-y-2">
						<Skeleton className="h-4 w-2/5" />
						<Skeleton className="h-9 w-full rounded-lg" />
						<Skeleton className="h-9 w-full rounded-lg" />
					</div>
				) : empty ? (
					<p className="rounded-lg border border-line bg-surface px-3.5 py-3 text-sm text-fg-subtle">
						Nothing to reset: no mods in this game folder.
					</p>
				) : (
					<div className="space-y-3">
						{preview.mods > 0 && (
							<p className="text-sm text-fg-muted">
								<span className="text-fg">
									{preview.mods} mod{preview.mods > 1 ? "s" : ""}
								</span>{" "}
								from Unalia
							</p>
						)}
						{preview.foreign.length > 0 && (
							<div className="space-y-1.5">
								<p className="text-xs font-medium tracking-wide text-fg-subtle uppercase">From other tools</p>
								<ul className="max-h-56 divide-y divide-line overflow-y-auto rounded-lg border border-line bg-surface">
									{preview.foreign.map((f) => (
										<li key={f.path} className="flex items-center gap-2.5 px-3.5 py-2 text-xs">
											{f.dir ? (
												<Folder className="size-3.5 shrink-0 text-fg-faint" />
											) : (
												<FileText className="size-3.5 shrink-0 text-fg-faint" />
											)}
											<span className="min-w-0 flex-1 truncate font-tight text-fg-muted" title={f.path}>
												{f.path}
												{f.dir && "/"}
											</span>
											<span className="shrink-0 font-tight text-fg-faint tabular-nums">{describe(f.dir, f.files, f.bytes)}</span>
										</li>
									))}
								</ul>
							</div>
						)}
					</div>
				)}

				<DialogFooter>
					<Button variant="secondary" disabled={resetting} onClick={() => setOpen(false)}>
						Cancel
					</Button>
					<Button variant="danger" isLoading={resetting} disabled={preview === null || empty} onClick={() => void reset()}>
						Reset mods
					</Button>
				</DialogFooter>
			</Dialog>
		</>
	);
}

function describe(dir: boolean, files: number, size: number): string {
	if (!dir) return bytes(size);
	if (files === 0) return "empty folder";
	return `${files} file${files > 1 ? "s" : ""} · ${bytes(size)}`;
}

function summary(mods: number, foreign: number): string {
	const parts = [];
	if (mods > 0) parts.push(`${mods} mod${mods > 1 ? "s" : ""} taken out`);
	if (foreign > 0) parts.push(`${foreign} leftover${foreign > 1 ? "s" : ""} from other tools sent to the Recycle Bin`);
	return parts.length > 0 ? `${parts.join(", ")}.` : "There was nothing to remove.";
}
