"use client";

import { Badge, Button, cn } from "@modalia/ui";
import { Check, Files } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import type { NexusModFile } from "@/lib/engine";
import { bytes } from "@/lib/format";
import { settleFileChoice, useFileChoice } from "@/lib/installs";

const CATEGORY: Record<string, string> = {
	MAIN: "Main file",
	OPTIONAL: "Optional",
	UPDATE: "Update",
	MISCELLANEOUS: "Miscellaneous",
	OLD_VERSION: "Older version",
	ARCHIVED: "Archived",
};

/**
 * Choix des fichiers d'un mod Nexus qui en a plusieurs (variantes de
 * couleur, options). Le fichier conseillé est coché d'office ; les fichiers
 * se posent dans l'ordre de la liste, chacun par-dessus le précédent. Dans
 * un portail : `fixed` ne doit dépendre d'aucun parent transformé.
 */
export function FilePicker() {
	const choice = useFileChoice();
	const [picked, setPicked] = useState<number[]>([]);

	// Nouveau choix : le fichier conseillé (en tête) coché d'office.
	useEffect(() => {
		setPicked(choice?.files[0] ? [choice.files[0].fileId] : []);
	}, [choice]);

	useEffect(() => {
		if (!choice) return;
		const onKey = (e: KeyboardEvent) => e.key === "Escape" && settleFileChoice(null);
		window.addEventListener("keydown", onKey);
		const overflow = document.documentElement.style.overflow;
		document.documentElement.style.overflow = "hidden";
		return () => {
			window.removeEventListener("keydown", onKey);
			document.documentElement.style.overflow = overflow;
		};
	}, [choice]);

	if (!choice) return null;

	const toggle = (id: number) =>
		setPicked((current) => (current.includes(id) ? current.filter((f) => f !== id) : [...current, id]));
	// L'ordre de pose est celui de la liste, pas celui des clics.
	const ordered = choice.files.map((f) => f.fileId).filter((id) => picked.includes(id));

	return createPortal(
		<div className="fixed inset-0 z-[70] flex items-center justify-center p-4">
			<button
				type="button"
				aria-label="Close"
				onClick={() => settleFileChoice(null)}
				className="absolute inset-0 animate-fade-in cursor-default bg-black/60 backdrop-blur-sm"
			/>
			<div
				role="dialog"
				aria-modal="true"
				aria-label="Choose files"
				className="relative flex max-h-[85vh] w-full max-w-[560px] animate-fade-up flex-col overflow-hidden rounded-xl border border-white/8 bg-bg shadow-2xl shadow-black/60"
			>
				<div className="flex items-center gap-1.5 border-b border-white/5 px-5 py-3">
					<Files className="size-3.5 text-fg-faint" />
					<span className="truncate text-xs font-medium text-fg-subtle">{choice.name ?? "This mod"}</span>
				</div>

				<div className="px-5 pt-5">
					<h3 className="text-base font-semibold text-fg">Choose what to install</h3>
					<p className="mt-1 text-xs leading-relaxed text-fg-subtle">
						This mod comes in several files. They install in this order, each on top of the previous one: a color
						variant replaces the look of the main file.
					</p>
				</div>

				<ul className="mt-4 min-h-0 flex-1 space-y-1.5 overflow-y-auto px-5 pb-1">
					{choice.files.map((file) => (
						<FileRow
							key={file.fileId}
							file={file}
							checked={picked.includes(file.fileId)}
							order={ordered.length > 1 ? ordered.indexOf(file.fileId) + 1 : 0}
							onToggle={() => toggle(file.fileId)}
						/>
					))}
				</ul>

				<div className="mt-4 flex items-center justify-between gap-3 border-t border-white/5 px-4 py-3">
					<span className="text-[11px] text-fg-faint">
						{ordered.length > 1 ? "Free accounts: Nexus Mods asks for one click per file." : " "}
					</span>
					<div className="flex shrink-0 items-center gap-2">
						<Button type="button" variant="secondary" size="sm" onClick={() => settleFileChoice(null)}>
							Cancel
						</Button>
						<Button type="button" size="sm" disabled={ordered.length === 0} onClick={() => settleFileChoice(ordered)}>
							{ordered.length > 1 ? `Install ${ordered.length} files` : "Install"}
						</Button>
					</div>
				</div>
			</div>
		</div>,
		document.body,
	);
}

function FileRow({
	file,
	checked,
	order,
	onToggle,
}: {
	file: NexusModFile;
	checked: boolean;
	/** Rang de pose quand plusieurs fichiers sont cochés, sinon 0. */
	order: number;
	onToggle: () => void;
}) {
	const meta = [file.version && `v${file.version}`, file.size !== null && bytes(file.size)].filter(Boolean).join(" · ");
	return (
		<li>
			<button
				type="button"
				role="checkbox"
				aria-checked={checked}
				onClick={onToggle}
				className={cn(
					"flex w-full cursor-pointer items-start gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors",
					checked ? "border-accent/35 bg-accent/8" : "border-line bg-surface hover:border-line-strong",
				)}
			>
				<span
					className={cn(
						"mt-0.5 flex size-4 shrink-0 items-center justify-center rounded border text-[10px] font-semibold",
						checked ? "border-accent bg-accent text-bg" : "border-line-strong",
					)}
				>
					{checked && (order ? order : <Check className="size-3" strokeWidth={3} />)}
				</span>
				<span className="min-w-0 flex-1">
					<span className="flex items-center gap-2">
						<span className="truncate text-sm font-medium text-fg">{file.name}</span>
						<Badge>{CATEGORY[file.category] ?? file.category}</Badge>
					</span>
					{meta && <span className="mt-0.5 block font-mono text-[11px] text-fg-muted">{meta}</span>}
					{file.description && (
						<span className="mt-1 line-clamp-2 block text-xs text-fg-subtle">{file.description}</span>
					)}
				</span>
			</button>
		</li>
	);
}
