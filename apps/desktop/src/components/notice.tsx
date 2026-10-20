import type { ReactNode } from "react";

/** Message d'état (vide, erreur, indisponible) : ce qui se passe et quoi faire. */
export function Notice({
	icon,
	title,
	children,
	action,
	details,
}: {
	icon: ReactNode;
	title: string;
	children: ReactNode;
	action?: ReactNode;
	details?: string;
}) {
	return (
		<div className="flex animate-row-in gap-3 px-3 py-3.5">
			<span className="flex size-10 shrink-0 items-center justify-center rounded-xl border border-line bg-surface">
				{icon}
			</span>
			<div className="min-w-0 flex-1">
				<p className="text-sm font-medium text-fg">{title}</p>
				<p className="mt-0.5 text-xs leading-relaxed text-fg-subtle">{children}</p>
				{action && <div className="mt-3">{action}</div>}
				{details && (
					<details className="mt-3 text-xs text-fg-faint">
						<summary className="cursor-pointer select-none transition-colors hover:text-fg-subtle">
							Technical details
						</summary>
						<pre className="mt-2 overflow-x-auto whitespace-pre-wrap rounded-md border border-line bg-black/30 p-2.5 font-mono text-[11px] text-fg-subtle">
							{details}
						</pre>
					</details>
				)}
			</div>
		</div>
	);
}
