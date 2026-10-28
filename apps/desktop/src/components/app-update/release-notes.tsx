"use client";

import { cn } from "@modalia/ui";
import { Fragment, useMemo } from "react";

import { type ChangeKind, parseReleaseNotes } from "@/lib/release-notes";

const KIND: Record<ChangeKind, { label: string; className: string }> = {
	new: { label: "New", className: "border-success/20 bg-success/10 text-success" },
	improved: { label: "Improved", className: "border-info/20 bg-info/10 text-info" },
	fixed: { label: "Fixed", className: "border-warning/20 bg-warning/10 text-warning" },
	removed: { label: "Removed", className: "border-danger/20 bg-danger/10 text-danger" },
};

/** Notes de version en liste, chaque changement avec son type en badge. */
export function ReleaseNotesList({ notes, className }: { notes: string; className?: string }) {
	const { intro, changes } = useMemo(() => parseReleaseNotes(notes), [notes]);

	return (
		<div className={cn("overflow-y-auto overscroll-contain", className)}>
			{intro.map((p) => (
				<p key={p} className="mb-3 text-sm text-fg-muted">
					<Inline text={p} />
				</p>
			))}
			{changes.length > 0 && (
				<ul className="flex flex-col gap-3">
					{changes.map((change, i) => (
						<li key={i} className="flex items-start gap-3">
							<span
								className={cn(
									"mt-0.5 w-[4.5rem] shrink-0 rounded-full border py-0.5 text-center text-[10px] font-semibold tracking-wider uppercase",
									KIND[change.kind].className,
								)}
							>
								{KIND[change.kind].label}
							</span>
							<p className="text-sm leading-relaxed text-fg/90">
								<Inline text={change.text} />
							</p>
						</li>
					))}
				</ul>
			)}
		</div>
	);
}

/** Texte des notes : seuls `code` et **gras** sont mis en forme. */
function Inline({ text }: { text: string }) {
	return text.split(/(`[^`]+`|\*\*[^*]+\*\*)/).map((part, i) => (
		<Fragment key={i}>
			{part.startsWith("`") && part.endsWith("`") && part.length > 1 ? (
				<code className="rounded bg-surface-strong px-1 py-px font-mono text-[0.85em] text-fg">{part.slice(1, -1)}</code>
			) : part.startsWith("**") && part.endsWith("**") && part.length > 4 ? (
				<strong className="font-semibold text-fg">{part.slice(2, -2)}</strong>
			) : (
				part
			)}
		</Fragment>
	));
}
