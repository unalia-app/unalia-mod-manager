"use client";

import { Button, cn } from "@modalia/ui";
import { ArrowUp, Check, Clock, Download, ExternalLink, RotateCw } from "lucide-react";

import { type InstallProgress, cancelPending } from "@/lib/engine";
import { bytes, timeLeft } from "@/lib/format";
import { usePendingOp } from "@/lib/game-status";
import { clearError, install, overallProgress, useActiveInstall, useInstalled, useModUpdate } from "@/lib/installs";
import { openExternal } from "@/lib/open";

/**
 * Un clic : le bouton devient lui-même la barre de progression
 * (téléchargement → installation), puis « Installed ». Jamais de popup.
 */
export function InstallButton({
	appId,
	modId,
	modName,
	dependencyCount,
	hint,
}: {
	appId: number;
	modId: string;
	modName: string;
	dependencyCount: number;
	/** Précision sous le bouton (ex. confirmation sur Nexus pour un compte gratuit). */
	hint?: string;
}) {
	const installed = useInstalled(appId)?.find((m) => m.id === modId);
	const active = useActiveInstall(appId, modId);
	const pending = usePendingOp(appId, modId);
	const update = useModUpdate(appId, modId);

	// Le résultat (réussite ou erreur) arrive par le store, que le bouton suit.
	const start = () => void install(appId, modId);

	if (active?.status === "queued" || active?.status === "running") {
		const progress = active.status === "running" ? active.progress : null;
		const speed = active.status === "running" ? active.speed : undefined;
		const value = progress ? overallProgress(progress) : 0;
		return (
			<div className="flex flex-col items-end gap-2">
				<div
					role="progressbar"
					aria-valuemin={0}
					aria-valuemax={100}
					aria-valuenow={Math.round(value * 100)}
					aria-label={`Installing ${modName}`}
					className="relative h-10 w-48 overflow-hidden rounded-md border border-line-strong bg-surface-strong"
				>
					<div
						className="absolute inset-y-0 left-0 w-full origin-left bg-accent/35 transition-[scale] duration-300 ease-out"
						style={{ scale: `${Math.max(value, 0.02)} 1` }}
					/>
					<div className="absolute inset-y-0 w-1/3 animate-shimmer bg-gradient-to-r from-transparent via-white/10 to-transparent" />
					<span className="relative flex h-full items-center justify-center gap-2 px-3 text-xs font-medium text-fg">
						{label(progress, value)}
					</span>
				</div>
				<span className="text-right text-[11px] whitespace-pre-line text-fg-subtle">{sublabel(progress, speed)}</span>
			</div>
		);
	}

	// Compte Nexus gratuit : Nexus veut un clic sur son site. Le lien
	// `nxm://` revient à Modalia, qui enchaîne tout seul.
	if (active?.status === "website") {
		return (
			<div className="flex max-w-64 flex-col items-end gap-2 text-right">
				<Button
					size="lg"
					variant="outline"
					leftIcon={<ExternalLink className="size-4 text-accent" />}
					onClick={() => void openExternal(active.url)}
				>
					Finish on Nexus Mods
				</Button>
				<span className="text-[11px] leading-snug text-fg-subtle">
					Click <span className="text-fg-muted">Slow download</span> on the page that opened. Modalia installs it right after ·{" "}
					<button
						type="button"
						className="cursor-pointer text-fg-muted underline-offset-2 hover:underline"
						onClick={() => clearError(appId, modId)}
					>
						Cancel
					</button>
				</span>
			</div>
		);
	}

	// Le jeu tourne : l'installation attend sa fermeture (SPEC §3.6).
	if (pending?.type === "install") {
		return (
			<div className="flex flex-col items-end gap-2">
				<Button size="lg" variant="outline" disabled leftIcon={<Clock className="size-4 text-accent" />} className="disabled:opacity-100">
					Queued
				</Button>
				<span className="text-[11px] text-fg-subtle">
					{pending.ready ? "Downloaded · installs when you close the game" : "Downloading in the background"} ·{" "}
					<button
						type="button"
						className="cursor-pointer text-fg-muted underline-offset-2 hover:underline"
						onClick={() => void cancelPending(appId, modId)}
					>
						Cancel
					</button>
				</span>
			</div>
		);
	}

	// Pas de mise à jour automatique (SPEC §3.5) : on propose, on n'impose pas.
	if (installed && update) {
		return (
			<div className="flex flex-col items-end gap-2">
				<Button size="lg" leftIcon={<ArrowUp className="size-4" />} onClick={start}>
					{`Update to v${update.latest}`}
				</Button>
				<span className="text-[11px] text-fg-subtle">You have v{update.current}</span>
			</div>
		);
	}

	if (installed) {
		return (
			<div className="flex flex-col items-end gap-2">
				<Button size="lg" variant="outline" disabled leftIcon={<Check className="size-4 text-success" />} className="disabled:opacity-100">
					Installed
				</Button>
				<span className="text-[11px] text-fg-subtle">
					v{installed.version}
					{installed.enabled ? "" : " · turned off"}
				</span>
			</div>
		);
	}

	if (active?.status === "error") {
		return (
			<div className="flex max-w-64 flex-col items-end gap-2 text-right">
				<Button
					size="lg"
					leftIcon={<RotateCw className="size-4" />}
					onClick={() => {
						clearError(appId, modId);
						start();
					}}
				>
					Try again
				</Button>
				<span className="text-[11px] leading-snug text-danger">{active.message}</span>
				{active.details && (
					<details className="text-[11px] text-fg-faint">
						<summary className="cursor-pointer select-none hover:text-fg-subtle">Technical details</summary>
						<p className="mt-1 font-mono break-words text-fg-subtle">{active.details}</p>
					</details>
				)}
			</div>
		);
	}

	return (
		<div className="flex flex-col items-end gap-2">
			<Button size="lg" leftIcon={<Download className="size-4" />} onClick={start}>
				Install
			</Button>
			{dependencyCount > 0 && (
				<span className="text-[11px] text-fg-subtle">
					+ {dependencyCount} {dependencyCount > 1 ? "dependencies" : "dependency"}
				</span>
			)}
			{hint && <span className="max-w-64 text-right text-[11px] leading-snug text-fg-faint">{hint}</span>}
		</div>
	);
}

/** Le pourcentage est global, comme la barre ; le paquet en cours est dessous. */
function label(progress: InstallProgress | null, value: number): string {
	if (!progress) return "Waiting…";
	if (progress.stage === "resolving") return "Preparing…";
	const percent = Math.round(value * 100);
	if (progress.stage === "downloading") return `Downloading ${percent}%`;
	if (progress.stage === "extracting") return `Unpacking ${percent}%`;
	return `Installing ${percent}%`;
}

/**
 * Paquet en cours, et pour les gros fichiers la quantité, la vitesse et le
 * temps restant : « 3.2 GB of 10 GB · 24 MB/s · 5 min left ».
 */
function sublabel(progress: InstallProgress | null, speed?: number): string {
	if (!progress || progress.stage === "resolving") return "Checking dependencies";
	const which = progress.total > 1 ? `${progress.name} · ${progress.index + 1} of ${progress.total}` : progress.name;
	if (progress.stage === "downloading" && progress.size && progress.size > 20 * 1024 * 1024) {
		const parts = [`${bytes(progress.received)} of ${bytes(progress.size)}`];
		if (speed && speed > 0) {
			parts.push(`${bytes(speed)}/s`, timeLeft((progress.size - progress.received) / speed));
		}
		return `${which}\n${parts.filter(Boolean).join(" · ")}`;
	}
	if (progress.stage === "extracting" && progress.size > 20 * 1024 * 1024) {
		return `${which}\nUnpacking ${bytes(progress.done)} of ${bytes(progress.size)}`;
	}
	return which;
}

/** Badge des cartes : installé / en cours. */
export function InstallStateBadge({ appId, modId, className }: { appId: number; modId: string; className?: string }) {
	const installed = useInstalled(appId)?.some((m) => m.id === modId);
	const active = useActiveInstall(appId, modId);
	const pending = usePendingOp(appId, modId)?.type === "install";
	const update = useModUpdate(appId, modId);
	const running = active?.status === "queued" || active?.status === "running";
	if (!installed && !running && !pending) return null;
	const accent = running || pending || Boolean(update);
	return (
		<span
			className={cn(
				"liquid-glass inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-tight text-[11px] font-semibold tracking-wide",
				accent ? "text-accent" : "text-success",
				className,
			)}
		>
			{pending ? "Queued" : running ? "Installing…" : update ? (
				<>
					<ArrowUp className="size-3" />
					Update
				</>
			) : (
				<>
					<Check className="size-3" />
					Installed
				</>
			)}
		</span>
	);
}
