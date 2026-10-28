import type { ComponentProps } from "react";

import { cn } from "./cn";

// Loading block with a sweeping sheen, like Unalia's cards.
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
