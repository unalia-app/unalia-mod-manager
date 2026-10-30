"use client";

import { Badge, Button, Dialog, DialogDescription, DialogFooter, DialogHeader, DialogTitle, cn } from "@modalia/ui";
import { ClipboardPaste, Share2 } from "lucide-react";
import { useState } from "react";

import { toast } from "@/components/toaster";
import { type InstalledMod, type SteamGame, splitError } from "@/lib/engine";
import { install, toggle } from "@/lib/installs";
import { type SharedList, type SharedMod, decodeList, encodeList, modsToShare } from "@/lib/share";

/** Copies the code of the mods turned on, for friends to paste in Import. */
export function ShareButton({ game, mods }: { game: SteamGame; mods: InstalledMod[] }) {
	const shared = modsToShare(mods);
	const share = async () => {
		if (!game.profile) return;
		try {
			const code = await encodeList({ game: game.profile.id, gameName: game.name, mods: shared });
			await navigator.clipboard.writeText(code);
			toast({
				title: "Mod list copied",
				details: `Send it to your friends: in My mods, Import then paste it to get the same ${shared.length} mod${shared.length === 1 ? "" : "s"}.`,
				duration: 8000,
			});
		} catch (error) {
			toast({ title: splitError(error).message, tone: "error" });
		}
	};
	return (
		<Button
			size="sm"
			variant="outline"
			leftIcon={<Share2 className="size-3.5" />}
			disabled={shared.length === 0 || !game.profile}
			title={shared.length === 0 ? "Turn on a mod to share your list" : "Copy a code your friends paste to get the same mods"}
			onClick={() => void share()}
		>
			Share
		</Button>
	);
}

/** What importing does to one mod of the list, on this player's side. */
type Step = { mod: SharedMod; action: "install" | "update" | "enable" | "none"; yours: string | null };

function plan(list: SharedList, installed: InstalledMod[]): Step[] {
	return list.mods.map((mod) => {
		const mine = installed.find((m) => m.id === mod.id);
		if (!mine) return { mod, action: "install", yours: null };
		if (!mine.enabled) return { mod, action: "enable", yours: mine.version };
		if (mine.version !== mod.version) return { mod, action: "update", yours: mine.version };
		return { mod, action: "none", yours: mine.version };
	});
}

const LABEL: Record<Step["action"], string> = {
	install: "To install",
	update: "To update",
	enable: "To turn on",
	none: "Installed",
};

/** Paste a friend's code: what's missing is installed, what's off is turned on. */
export function ImportButton({
	game,
	mods,
	variant = "outline",
}: {
	game: SteamGame;
	mods: InstalledMod[];
	variant?: "outline" | "secondary";
}) {
	const [open, setOpen] = useState(false);
	return (
		<>
			<Button size="sm" variant={variant} leftIcon={<ClipboardPaste className="size-3.5" />} onClick={() => setOpen(true)}>
				Import
			</Button>
			{open && <ImportDialog game={game} mods={mods} onClose={() => setOpen(false)} />}
		</>
	);
}

function ImportDialog({ game, mods, onClose }: { game: SteamGame; mods: InstalledMod[]; onClose: () => void }) {
	const [code, setCode] = useState("");
	const [list, setList] = useState<SharedList | null>(null);
	const [error, setError] = useState<string | null>(null);

	const read = async (text: string) => {
		setCode(text);
		if (!text.trim()) return setError(null);
		const decoded = await decodeList(text);
		setError(decoded ? null : "This isn't a mod list code. Ask your friend to copy it again with Share.");
		setList(decoded);
	};

	const otherGame = list && list.game !== game.profile?.id;
	const steps = list && !otherGame ? plan(list, mods) : [];
	const todo = steps.filter((s) => s.action !== "none");
	const extra = list && !otherGame ? mods.filter((m) => m.explicit && m.loaderRoot === null && !list.mods.some((s) => s.id === m.id)) : [];
	const nexus = !!game.profile?.nexus && todo.some((s) => s.action === "install" || s.action === "update");

	const apply = () => {
		for (const { mod, action } of todo) {
			if (action === "enable") {
				toggle(game.appId, mod.id, true).catch((e: unknown) => toast({ title: splitError(e).message, tone: "error" }));
			} else {
				void install(game.appId, mod.id, mod.name, mod.files.length ? mod.files : undefined, mod.source || undefined);
			}
		}
		toast({ title: `Getting ${todo.length} mod${todo.length === 1 ? "" : "s"} from the list`, details: "Follow them in My mods." });
		onClose();
	};

	return (
		<Dialog open onClose={onClose} className="max-w-lg">
			<DialogHeader>
				<DialogTitle>Import a mod list</DialogTitle>
				<DialogDescription>
					Paste the code a friend copied with Share. Unalia installs what you're missing, so you play with the same mods.
				</DialogDescription>
			</DialogHeader>

			{!list ? (
				<div className="space-y-2">
					<textarea
						value={code}
						onChange={(e) => void read(e.target.value)}
						placeholder="unalia-mods:…"
						rows={4}
						// The dialog opens to paste: the cursor is already there.
						autoFocus
						spellCheck={false}
						className="w-full resize-none rounded-lg border border-line bg-surface px-3 py-2 font-mono text-xs break-all text-fg/80 outline-none placeholder:text-white/22 focus:border-line-strong"
					/>
					{error && <p className="text-xs text-danger">{error}</p>}
				</div>
			) : otherGame ? (
				<p className="text-sm text-fg-muted">
					This list is for <span className="text-fg">{list.gameName || "another game"}</span>. Open that game in Unalia, then import it
					there.
				</p>
			) : (
				<div className="space-y-3">
					{todo.length === 0 && <p className="text-sm text-fg-muted">You already have every mod of this list.</p>}
					<ul className="max-h-72 overflow-y-auto rounded-xl border border-line bg-surface">
						{steps.map(({ mod, action, yours }, i) => (
							<li key={mod.id} className={cn("flex items-center gap-3 px-4 py-2.5", i > 0 && "border-t border-line")}>
								<span className="min-w-0 flex-1">
									<span className="block truncate text-sm text-fg">{mod.name}</span>
									<span className="block truncate font-mono text-[11px] text-fg-subtle">
										{mod.version && `v${mod.version}`}
										{yours && yours !== mod.version && ` · yours: v${yours}`}
									</span>
								</span>
								{action === "none" ? <Badge>{LABEL[action]}</Badge> : <Badge tone="accent">{LABEL[action]}</Badge>}
							</li>
						))}
					</ul>
					{extra.length > 0 && (
						<p className="text-xs text-fg-subtle">
							You also have {extra.length} mod{extra.length === 1 ? "" : "s"} that {extra.length === 1 ? "isn't" : "aren't"} in this list:{" "}
							{extra.length === 1 ? "it stays" : "they stay"}. Turn {extra.length === 1 ? "it" : "them"} off in My mods if your
							friends don't have {extra.length === 1 ? "it" : "them"}.
						</p>
					)}
					{nexus && (
						<p className="text-xs text-fg-subtle">
							Free Nexus Mods account: each mod opens its page, click <span className="text-fg-muted">Slow download</span> there.
						</p>
					)}
				</div>
			)}

			<DialogFooter>
				<Button size="sm" variant="secondary" onClick={onClose}>
					{list && !otherGame && todo.length === 0 ? "Close" : "Cancel"}
				</Button>
				{list && !otherGame && todo.length > 0 && (
					<Button size="sm" onClick={apply}>
						{`Get ${todo.length} mod${todo.length === 1 ? "" : "s"}`}
					</Button>
				)}
			</DialogFooter>
		</Dialog>
	);
}
