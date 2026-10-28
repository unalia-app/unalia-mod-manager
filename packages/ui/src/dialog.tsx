"use client";
// Unalia's AlertDialog (components/ui/shadcn/alert-dialog.tsx), with the
// @modalia/ui tokens and without Radix: title, description, actions bottom right.

import { type ComponentProps, type ReactNode, createContext, useContext, useEffect, useId, useRef } from "react";
import { createPortal } from "react-dom";

import { cn } from "./cn";

const TitleId = createContext<string | undefined>(undefined);

/**
 * Modal dialog. Escape or a click outside closes it (`onClose`), the page
 * behind stops scrolling, and focus returns where it was on close. In a
 * portal: `fixed` must not depend on any transformed parent.
 */
export function Dialog({
	open,
	onClose,
	children,
	className,
}: {
	open: boolean;
	/** Missing: nothing closes the dialog (action in progress). */
	onClose?: () => void;
	children: ReactNode;
	className?: string;
}) {
	const titleId = useId();
	const panel = useRef<HTMLDivElement>(null);

	useEffect(() => {
		if (!open) return;
		const before = document.activeElement as HTMLElement | null;
		// Focus enters the dialog, unless a field already took it.
		if (!panel.current?.contains(document.activeElement)) panel.current?.focus();
		const overflow = document.documentElement.style.overflow;
		document.documentElement.style.overflow = "hidden";
		return () => {
			document.documentElement.style.overflow = overflow;
			before?.focus();
		};
	}, [open]);

	useEffect(() => {
		if (!open || !onClose) return;
		const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [open, onClose]);

	if (!open) return null;

	return createPortal(
		<div className="fixed inset-0 z-[70] flex items-center justify-center p-4">
			<button
				type="button"
				aria-label="Close"
				tabIndex={-1}
				onClick={onClose}
				className="absolute inset-0 animate-fade-in cursor-default bg-black/60 backdrop-blur-sm"
			/>
			<div
				ref={panel}
				role="alertdialog"
				aria-modal="true"
				aria-labelledby={titleId}
				tabIndex={-1}
				className={cn(
					"relative grid w-full max-w-md animate-dialog-in gap-5 rounded-2xl border border-white/8 bg-bg p-6 shadow-2xl shadow-black/60 outline-none",
					className,
				)}
			>
				<TitleId.Provider value={titleId}>{children}</TitleId.Provider>
			</div>
		</div>,
		document.body,
	);
}

export function DialogHeader({ className, ...props }: ComponentProps<"div">) {
	return <div className={cn("flex flex-col gap-2", className)} {...props} />;
}

export function DialogTitle({ className, ...props }: ComponentProps<"h2">) {
	return <h2 id={useContext(TitleId)} className={cn("text-lg font-semibold tracking-tight text-fg", className)} {...props} />;
}

export function DialogDescription({ className, ...props }: ComponentProps<"p">) {
	return <p className={cn("text-sm leading-relaxed text-fg-muted", className)} {...props} />;
}

export function DialogFooter({ className, ...props }: ComponentProps<"div">) {
	return <div className={cn("flex flex-col-reverse gap-2 sm:flex-row sm:justify-end", className)} {...props} />;
}
