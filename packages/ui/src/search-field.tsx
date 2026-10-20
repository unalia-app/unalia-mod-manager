import { SearchIcon } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";

import { cn } from "./cn";

export type SearchFieldProps = Omit<ComponentProps<"input">, "type"> & {
	/** Élément affiché à droite (raccourci clavier, compteur…). */
	trailing?: ReactNode;
};

// Champ de recherche d'Unalia (verre clair, léger soulèvement au survol),
// ici en vrai `<input>`.
export function SearchField({ className, trailing, ...props }: SearchFieldProps) {
	return (
		<label
			className={cn(
				"group flex h-12 w-full items-center gap-3 rounded-lg border border-line-strong bg-surface-strong px-4 shadow-sm transition-all duration-200",
				"hover:-translate-y-px hover:bg-white/15 hover:shadow-lg",
				"focus-within:border-white/30 focus-within:ring-2 focus-within:ring-white/20",
				className,
			)}
		>
			<SearchIcon className="-ms-1 size-5 shrink-0 text-fg/80" aria-hidden />
			<input
				type="search"
				autoComplete="off"
				spellCheck={false}
				className="h-full w-full min-w-0 bg-transparent text-sm font-medium text-fg outline-none placeholder:text-fg-muted [&::-webkit-search-cancel-button]:hidden"
				{...props}
			/>
			{trailing}
		</label>
	);
}
