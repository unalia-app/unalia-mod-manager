"use client";
// AlertDialog d'Unalia (components/ui/shadcn/alert-dialog.tsx), aux tokens
// de @modalia/ui et sans Radix : titre, description, actions en bas à droite.

import { type ComponentProps, type ReactNode, createContext, useContext, useEffect, useId, useRef } from "react";
import { createPortal } from "react-dom";

import { cn } from "./cn";

const TitleId = createContext<string | undefined>(undefined);

/**
 * Fenêtre modale. Échap ou un clic à côté la ferment (`onClose`), la page
 * derrière ne défile plus, et le focus revient où il était à la fermeture.
 * Dans un portail : `fixed` ne doit dépendre d'aucun parent transformé.
 */
export function Dialog({
	open,
	onClose,
	children,
	className,
}: {
	open: boolean;
	/** Absent : rien ne ferme la fenêtre (action en cours). */
	onClose?: () => void;
	children: ReactNode;
	className?: string;
}) {
	const titleId = useId();
	const panel = useRef<HTMLDivElement>(null);

	useEffect(() => {
		if (!open) return;
		const before = document.activeElement as HTMLElement | null;
		// Le focus entre dans la fenêtre, sauf si un champ l'a déjà pris.
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
