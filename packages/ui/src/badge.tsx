import { type VariantProps, cva } from "class-variance-authority";
import type { ComponentProps } from "react";

import { cn } from "./cn";

// Outline + 10% background of the hue, like Unalia's NEW / IMPROVED badges.
const badgeVariants = cva(
	"inline-flex w-fit shrink-0 items-center gap-1 whitespace-nowrap rounded-full border px-2 py-0.5 font-tight text-[11px] font-semibold tracking-wide [&>svg]:size-3",
	{
		variants: {
			tone: {
				neutral: "border-line-strong bg-surface-strong text-fg-muted",
				accent: "border-accent/20 bg-accent/10 text-accent",
				success: "border-success/20 bg-success/10 text-success",
				info: "border-info/20 bg-info/10 text-info",
				warning: "border-warning/20 bg-warning/10 text-warning",
				danger: "border-danger/20 bg-danger/10 text-danger",
			},
		},
		defaultVariants: { tone: "neutral" },
	},
);

export type BadgeProps = ComponentProps<"span"> & VariantProps<typeof badgeVariants>;

export function Badge({ className, tone, ...props }: BadgeProps) {
	return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}

export { badgeVariants };
