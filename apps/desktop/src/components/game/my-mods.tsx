"use client";

import { Badge, Button, Skeleton, Switch, cn } from "@modalia/ui";
import { Layers, Package, Trash2 } from "lucide-react";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { Notice } from "@/components/notice";
import { toast } from "@/components/toaster";
import { type InstalledMod, cancelPending, splitError } from "@/lib/engine";
import { usePendingOp } from "@/lib/game-status";
import { modHref } from "@/lib/games-store";
import { remove, restore, toggle, useInstalled } from "@/lib/installs";

/** Onglet Mes mods (SPEC §3.5) : activer, désactiver, retirer avec « Undo ». */
export function MyMods({ appId, onDiscover }: { appId: number; onDiscover: () => void }) {
	const mods = useInstalled(appId);

	if (mods === undefined) {
		return (
			<div className="max-w-3xl space-y-2">
				{[0, 1, 2].map((i) => (
					<Skeleton key={i} className="h-16 rounded-xl" />
				))}
			</div>
		);
	}

	if (mods.length === 0) {
		return (
			<div className="glass max-w-xl">
				<Notice
					icon={<Layers className="size-5 text-fg-muted" />}
					title="No mods installed yet"
					action={
						<Button size="sm" onClick={onDiscover}>
							Discover mods
						</Button>
					}
				>
					Mods you install show up here. Turn them on, off or remove them in one click.
				</Notice>
			</div>
		);
	}

	const active = mods.filter((m) => m.enabled && !m.loaderRoot).length;

	return (
		<div className="max-w-3xl">
			<p className="mb-3 text-xs text-fg-subtle">
				{active} active mod{active === 1 ? "" : "s"} · changes apply the next time you launch the game
			</p>
			<ul className="overflow-hidden rounded-xl border border-line bg-surface">
				{mods.map((m, i) => (
					<li key={m.id} className={cn(i > 0 && "border-t border-line")}>
						<ModRow mod={m} appId={appId} />
					</li>
				))}
			</ul>
		</div>
	);
}

function ModRow({ mod: m, appId }: { mod: InstalledMod; appId: number }) {
	const router = useRouter();
	const [busy, setBusy] = useState(false);
	const loader = m.loaderRoot !== null;
	// Changement demandé pendant une partie : on montre l'état prévu.
	const pending = usePendingOp(appId, m.id);
	const removing = pending?.type === "uninstall";
	const enabled = pending?.type === "setEnabled" ? pending.enabled : m.enabled;

	const fail = (error: unknown) => {
		const { message, details } = splitError(error);
		toast({ title: message, description: details ?? undefined, tone: "error" });
	};

	const onToggle = (value: boolean) => {
		toggle(appId, m.id, value).catch(fail);
	};

	const onRemove = async () => {
		setBusy(true);
		try {
			const outcome = await remove(appId, m.id);
			toast(
				outcome === "queued"
					? {
							title: `${m.name} will be removed when you close the game`,
							action: { label: "Undo", onClick: () => cancelPending(appId, m.id).catch(fail) },
						}
					: {
							title: `${m.name} removed`,
							action: { label: "Undo", onClick: () => restore(appId, m.id).catch(fail) },
						},
			);
		} catch (error) {
			fail(error);
		} finally {
			setBusy(false);
		}
	};

	return (
		<div className={cn("flex animate-row-in items-center gap-3 px-4 py-3", removing && "opacity-60")}>
			<button
				type="button"
				onClick={() => router.push(modHref(appId, m.id))}
				className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left"
			>
				<ModThumb src={m.iconUrl} dimmed={!enabled || removing} />
				<span className="min-w-0">
					<span className="flex items-center gap-2">
						<span className={cn("truncate text-sm font-medium", enabled && !removing ? "text-fg" : "text-fg-subtle")}>
							{m.name}
						</span>
						{loader && <Badge tone="accent">Loader</Badge>}
						{!loader && !m.explicit && <Badge>Dependency</Badge>}
						{pending && <Badge tone="accent">Pending</Badge>}
					</span>
					<span className="block truncate text-xs text-fg-subtle">
						by {m.author} · v{m.version}
					</span>
				</span>
			</button>

			{removing ? (
				<span className="text-[11px] text-fg-subtle">Removed when you close the game</span>
			) : loader ? (
				<span className="text-[11px] text-fg-faint">Required by your mods</span>
			) : (
				<Switch checked={enabled} onChange={onToggle} label={`${enabled ? "Turn off" : "Turn on"} ${m.name}`} />
			)}
			<button
				type="button"
				aria-label={`Remove ${m.name}`}
				title="Remove"
				disabled={busy || removing}
				onClick={onRemove}
				className="cursor-pointer rounded-md p-2 text-fg-subtle transition-colors hover:bg-danger/10 hover:text-danger disabled:opacity-50"
			>
				<Trash2 className="size-4" />
			</button>
		</div>
	);
}

function ModThumb({ src, dimmed }: { src: string | null; dimmed: boolean }) {
	const [failed, setFailed] = useState(false);
	return (
		<span
			className={cn(
				"flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-line bg-white/[0.04]",
				dimmed && "opacity-50 grayscale",
			)}
		>
			{src && !failed ? (
				<img src={src} alt="" className="size-full object-cover" onError={() => setFailed(true)} />
			) : (
				<Package className="size-4 text-fg-faint" />
			)}
		</span>
	);
}
