"use client";

import { Button, Spinner } from "@modalia/ui";
import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { Download, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";

import { checkForUpdate, installUpdate, useAppUpdate } from "@/lib/app-update";

/** Version installée, et la mise à jour de l'appli à portée de clic. */
export function AppVersion() {
	const update = useAppUpdate();
	const [version, setVersion] = useState<string | null>(null);

	useEffect(() => {
		let current = true;
		(isTauri() ? getVersion() : Promise.resolve("dev")).then((v) => current && setVersion(v));
		return () => {
			current = false;
		};
	}, []);

	// Les erreurs restent affichées dans la carte : rien d'autre à faire ici.
	const check = () => checkForUpdate().catch(() => {});
	const install = () => installUpdate().catch(() => {});

	const available = update.status === "available" || (update.status === "error" && update.update);
	const target = "update" in update ? update.update : null;

	return (
		<div className="rounded-xl border border-line bg-white/[0.03] p-5 sm:p-6">
			<h3 className="text-sm font-semibold text-fg">Unalia Mod Manager {version ?? ""}</h3>
			<p className="mt-0.5 max-w-xl text-xs leading-relaxed text-fg-subtle">{describe(update)}</p>
			{update.status === "error" && update.details && (
				<details className="mt-2 text-xs text-fg-faint">
					<summary className="cursor-pointer select-none hover:text-fg-subtle">Technical details</summary>
					<p className="mt-2 rounded-lg border border-line bg-surface px-3 py-2 font-mono break-words text-fg-subtle">
						{update.details}
					</p>
				</details>
			)}
			<div className="mt-4 flex flex-wrap items-center gap-2">
				{update.status === "installing" ? (
					<span className="flex items-center gap-2 text-sm text-fg-muted">
						<Spinner className="size-3.5" />
						{update.progress === null
							? "Downloading…"
							: `Downloading… ${Math.round(update.progress * 100)}%`}
					</span>
				) : available && target ? (
					<Button leftIcon={<Download className="size-3.5" />} onClick={install}>
						{`Update to ${target.version}`}
					</Button>
				) : (
					<Button
						variant="outline"
						leftIcon={<RefreshCw className="size-3.5" />}
						onClick={check}
						isLoading={update.status === "checking"}
					>
						Check for updates
					</Button>
				)}
			</div>
		</div>
	);
}

function describe(update: ReturnType<typeof useAppUpdate>): string {
	switch (update.status) {
		case "idle":
		case "checking":
			return "Updates are checked each time the app opens.";
		case "upToDate":
			return "You have the latest version.";
		case "available":
			return `Version ${update.update.version} is available. The app restarts to finish, your mods stay as they are.`;
		case "installing":
			return "The app will close and reopen on the new version.";
		case "error":
			return update.message;
	}
}
