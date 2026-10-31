"use client";

import { Badge, Button, cn } from "@modalia/ui";
import { Check, Files } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { Marquee } from "@/components/marquee";
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
 * File picker for a Nexus mod that has several (versions, options).
 * Several main files are versions of the same mod: only one can be picked
 * (the engine puts first the one that works with what Unalia installs).
 * Options add on top. The advised file is ticked by default; files are
 * placed in list order, each over the previous one. In a portal: `fixed`
 * must not depend on any transformed parent.
 */
export function FilePicker() {
	const choice = useFileChoice();
	const [picked, setPicked] = useState<number[]>([]);

	// New choice: the advised file (first) ticked by default.
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

	const mains = choice.files.filter((f) => f.category === "MAIN").map((f) => f.fileId);
	const versions = mains.length > 1;
	const toggle = (id: number) =>
		setPicked((current) => {
			// A single version: picking one replaces the other.
			if (versions && mains.includes(id)) return [id, ...current.filter((f) => !mains.includes(f))];
			return current.includes(id) ? current.filter((f) => f !== id) : [...current, id];
		});
	// The placing order is the list's, not the clicks'.
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
				className="relative flex max-h-[85vh] w-full max-w-[720px] animate-fade-up flex-col overflow-hidden rounded-xl border border-white/8 bg-bg shadow-2xl shadow-black/60"
			>
				<div className="flex items-center gap-1.5 border-b border-white/5 px-5 py-3">
					<Files className="size-3.5 text-fg-faint" />
					<Marquee text={choice.name ?? "This mod"} className="text-xs font-medium text-fg-subtle" />
				</div>

				<div className="px-5 pt-5">
					<h3 className="text-base font-semibold text-fg">Choose what to install</h3>
					<p className="mt-1 text-xs leading-relaxed text-fg-subtle">
						{versions
							? "This mod comes in several versions: pick one. The recommended one works with what Unalia installs for you. Optional files add to it."
							: "This mod comes in several files. They install in this order, each on top of the previous one."}
					</p>
				</div>

				<ul className="mt-4 min-h-0 flex-1 space-y-1.5 overflow-y-auto px-5 pb-1">
					{choice.files.map((file) => (
						<FileRow
							key={file.fileId}
							file={file}
							single={versions && file.category === "MAIN"}
							recommended={versions && file.fileId === choice.files[0]?.fileId && file.category === "MAIN"}
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
	single,
	recommended,
	checked,
	order,
	onToggle,
}: {
	file: NexusModFile;
	/** One version among others: single choice (radio button). */
	single: boolean;
	recommended: boolean;
	checked: boolean;
	/** Placing rank when several files are ticked, else 0. */
	order: number;
	onToggle: () => void;
}) {
	const meta = [file.version && `v${file.version}`, file.size !== null && bytes(file.size)].filter(Boolean).join(" · ");
	return (
		<li>
			<button
				type="button"
				role={single ? "radio" : "checkbox"}
				aria-checked={checked}
				onClick={onToggle}
				className={cn(
					"flex w-full cursor-pointer items-start gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors",
					checked ? "border-accent/35 bg-accent/8" : "border-line bg-surface hover:border-line-strong",
				)}
			>
				{single ? (
					<span
						className={cn(
							"mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-full border",
							checked ? "border-accent" : "border-line-strong",
						)}
					>
						{checked && <span className="size-2 rounded-full bg-accent" />}
					</span>
				) : (
					<span
						className={cn(
							"mt-0.5 flex size-4 shrink-0 items-center justify-center rounded border text-[10px] font-semibold",
							checked ? "border-accent bg-accent text-bg" : "border-line-strong",
						)}
					>
						{checked && (order ? order : <Check className="size-3" strokeWidth={3} />)}
					</span>
				)}
				<span className="min-w-0 flex-1">
					<span className="flex items-center gap-2">
						<Marquee text={file.name} className="text-sm font-medium text-fg" />
						<Badge>{single ? "Version" : (CATEGORY[file.category] ?? file.category)}</Badge>
						{recommended && <Badge tone="success">Recommended</Badge>}
					</span>
					{meta && <span className="mt-0.5 block font-mono text-[11px] text-fg-muted">{meta}</span>}
					{file.description && (
						<span title={file.description} className="mt-1 line-clamp-2 block text-xs text-fg-subtle">
							{file.description}
						</span>
					)}
				</span>
			</button>
		</li>
	);
}
