import { type VariantProps, cva } from "class-variance-authority";
import type { ComponentProps, ReactNode } from "react";

import { cn } from "./cn";
import { Spinner } from "./spinner";

// Taken from Unalia's MainButton: white background, text that slides on hover.
const buttonVariants = cva(
	"group inline-flex select-none items-center justify-center rounded-md font-medium transition-[color,background-color,scale] duration-200 cursor-pointer active:scale-[0.98] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-60",
	{
		variants: {
			variant: {
				primary: "bg-white text-bg hover:bg-white/85 disabled:bg-white/30",
				secondary: "text-fg hover:bg-surface-strong",
				outline:
					"border border-line-strong bg-surface text-fg hover:bg-surface-hover",
				// Action that removes something: state color, never solid.
				danger: "border border-danger/30 bg-danger/10 text-danger hover:bg-danger/20",
			},
			size: {
				sm: "h-8 gap-1.5 px-3 text-xs",
				md: "h-9 gap-2 px-4 text-xs",
				lg: "h-10 gap-2.5 px-5 text-sm",
				icon: "size-9",
			},
		},
		defaultVariants: {
			variant: "primary",
			size: "md",
		},
	},
);

export type ButtonProps = ComponentProps<"button"> &
	VariantProps<typeof buttonVariants> & {
		leftIcon?: ReactNode;
		rightIcon?: ReactNode;
		isLoading?: boolean;
	};

export function Button({
	className,
	variant,
	size,
	leftIcon,
	rightIcon,
	isLoading = false,
	disabled,
	children,
	type = "button",
	...rest
}: ButtonProps) {
	const inactive = disabled || isLoading;

	return (
		<button
			type={type}
			className={cn(buttonVariants({ variant, size }), className)}
			disabled={inactive}
			aria-busy={isLoading || undefined}
			{...rest}
		>
			{isLoading ? (
				<Spinner />
			) : (
				<>
					{leftIcon && <span className="-ml-0.5 inline-flex">{leftIcon}</span>}
					{typeof children === "string" && !inactive ? (
						<SlidingLabel label={children} />
					) : (
						children
					)}
					{rightIcon && <span className="-mr-0.5 inline-flex">{rightIcon}</span>}
				</>
			)}
		</button>
	);
}

// The hidden copy starts at 200%: at 164% like in Unalia, the slant made
// the top of the capitals peek out under the label.
function SlidingLabel({ label }: { label: string }) {
	return (
		<span className="relative inline-flex overflow-hidden">
			<span className="transition duration-500 ease-out group-hover:-translate-y-[200%] group-hover:skew-y-12">
				{label}
			</span>
			<span
				aria-hidden
				className="absolute translate-y-[200%] skew-y-12 transition duration-500 ease-out group-hover:translate-y-0 group-hover:skew-y-0"
			>
				{label}
			</span>
		</span>
	);
}

export { buttonVariants };
