import { cn } from "./cn";

/** Interrupteur instantané (activer / désactiver un mod, SPEC §3.5). */
export function Switch({
	checked,
	onChange,
	label,
	disabled = false,
	className,
}: {
	checked: boolean;
	onChange: (checked: boolean) => void;
	/** Nom accessible. */
	label: string;
	disabled?: boolean;
	className?: string;
}) {
	return (
		<button
			type="button"
			role="switch"
			aria-checked={checked}
			aria-label={label}
			disabled={disabled}
			onClick={() => onChange(!checked)}
			className={cn(
				"relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border transition-colors duration-200 ease-out focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50",
				checked ? "border-transparent bg-accent" : "border-line-strong bg-surface-strong",
				className,
			)}
		>
			<span
				aria-hidden
				className={cn(
					"size-3.5 rounded-full shadow-sm transition-[translate,background-color] duration-200 ease-out",
					checked ? "translate-x-[18px] bg-bg" : "translate-x-[2px] bg-fg-muted",
				)}
			/>
		</button>
	);
}
