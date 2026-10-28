"use client";

import { Button, Dialog, DialogFooter, DialogHeader, DialogTitle } from "@modalia/ui";
import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { Sparkles } from "lucide-react";
import { useEffect, useState } from "react";

import { ReleaseNotesList } from "@/components/app-update/release-notes";
import { Glow } from "@/components/app-update-check";
import { SAMPLE_NOTES } from "@/lib/app-update";
import { takeWhatsNew } from "@/lib/whats-new";

/** Laisse la fenêtre se poser avant de s'ouvrir. */
const OPEN_AFTER_MS = 800;

/**
 * « What's new », comme dans Unalia : à la première ouverture après une
 * mise à jour faite depuis l'appli, les notes de la version, une fois.
 */
export function WhatsNew() {
	const [shown, setShown] = useState<{ version: string; notes: string } | null>(null);
	const [open, setOpen] = useState(false);

	useEffect(() => {
		if (process.env.NODE_ENV === "development") {
			// Aperçu en dev : Ctrl+Maj+N (`DevShortcuts`).
			const preview = () => {
				setShown({ version: "0.1.2", notes: SAMPLE_NOTES });
				setOpen(true);
			};
			window.addEventListener("unalia:preview-whats-new", preview);
			return () => window.removeEventListener("unalia:preview-whats-new", preview);
		}
		if (!isTauri()) return;
		let timer: number | undefined;
		getVersion().then((version) => {
			const notes = takeWhatsNew(version);
			if (notes === null) return;
			setShown({ version, notes });
			timer = window.setTimeout(() => setOpen(true), OPEN_AFTER_MS);
		});
		return () => window.clearTimeout(timer);
	}, []);

	if (!shown) return null;

	return (
		<Dialog open={open} onClose={() => setOpen(false)} className="grain max-w-lg bg-[#08080a]">
			<Glow className="from-info/15" />
			<DialogHeader>
				<div className="flex items-center gap-3">
					<span className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-info/20 bg-info/10">
						<Sparkles className="size-4 text-info" />
					</span>
					<DialogTitle>What's new</DialogTitle>
					<span className="rounded-full border border-info/20 bg-info/10 px-2 py-0.5 font-mono text-[11px] text-info">
						v{shown.version}
					</span>
				</div>
			</DialogHeader>

			<ReleaseNotesList notes={shown.notes} className="-mx-6 max-h-[55vh] border-y border-line px-6 py-4" />

			<DialogFooter>
				<Button onClick={() => setOpen(false)}>Got it</Button>
			</DialogFooter>
		</Dialog>
	);
}
