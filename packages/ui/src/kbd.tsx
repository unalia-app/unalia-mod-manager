import type { ComponentProps } from "react";

import { cn } from "./cn";

export function Kbd({ className, ...props }: ComponentProps<"kbd">) {
	return (
		<kbd
			className={cn(
				"inline-flex h-5 min-w-5 items-center justify-center rounded border border-line-strong bg-surface-strong px-1 font-tight text-[11px] font-medium text-fg-muted",
				className,
			)}
			{...props}
		/>
	);
}
