import type { ComponentProps } from "react";

import { cn } from "./cn";

// Bloc de chargement avec reflet qui balaie, comme les cartes d'Unalia.
export function Skeleton({ className, ...props }: ComponentProps<"div">) {
	return (
		<div
			aria-hidden
			className={cn("relative overflow-hidden rounded-md bg-white/[0.04]", className)}
			{...props}
		>
			<div className="absolute inset-y-0 w-1/2 animate-shimmer bg-gradient-to-r from-transparent via-white/[0.07] to-transparent" />
		</div>
	);
}
