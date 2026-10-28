"use client";

import { Button, Dialog, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@modalia/ui";
import { ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";

import type { SteamGame } from "@/lib/engine";

const ACKNOWLEDGED = "unalia.antiCheatAcknowledged";

function acknowledged(appId: number): boolean {
	try {
		const list: unknown = JSON.parse(localStorage.getItem(ACKNOWLEDGED) ?? "[]");
		return Array.isArray(list) && list.includes(appId);
	} catch {
		return false;
	}
}

function acknowledge(appId: number) {
	try {
		const list: unknown = JSON.parse(localStorage.getItem(ACKNOWLEDGED) ?? "[]");
		const ids = Array.isArray(list) ? list : [];
		localStorage.setItem(ACKNOWLEDGED, JSON.stringify([...ids, appId]));
	} catch {
		// Sans stockage, la fenêtre reviendra à la prochaine visite.
	}
}

/**
 * Jeu protégé par un anti-triche : les mods restent permis (c'est le jeu du
 * joueur), mais en ligne ils peuvent valoir un bannissement. Un bandeau sur
 * la page du jeu, toujours là, et une fenêtre à la première visite.
 */
export function AntiCheatBanner({ game }: { game: SteamGame }) {
	const [open, setOpen] = useState(false);

	useEffect(() => {
		if (game.antiCheat && !acknowledged(game.appId)) setOpen(true);
	}, [game.appId, game.antiCheat]);

	if (!game.antiCheat) return null;

	const close = () => {
		acknowledge(game.appId);
		setOpen(false);
	};

	return (
		<>
			<div className="mt-6 flex animate-row-in items-start gap-2 rounded-xl border border-danger/25 bg-danger/[0.07] px-4 py-3 text-sm">
				<ShieldAlert className="mt-0.5 size-4 shrink-0 text-danger" />
				<span>
					<span className="font-medium text-fg">This game uses {game.antiCheat}.</span>{" "}
					<span className="text-fg-muted">
						Mods can get your account banned online. Play offline or in single player only, at your own risk.
					</span>
				</span>
			</div>

			<Dialog open={open} onClose={close} className="max-w-md">
				<DialogHeader>
					<div className="flex items-center gap-3">
						<span className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-danger/20 bg-danger/10">
							<ShieldAlert className="size-4 text-danger" />
						</span>
						<DialogTitle>Before you mod {game.name}</DialogTitle>
					</div>
					<DialogDescription>
						{game.name} uses {game.antiCheat}. It can detect mods and ban your account, even for mods that only
						change visuals. Only play with mods offline or in single player, and turn them off before going online.
						Unalia can't protect you from a ban.
					</DialogDescription>
				</DialogHeader>
				<DialogFooter>
					<Button onClick={close}>I understand</Button>
				</DialogFooter>
			</Dialog>
		</>
	);
}
