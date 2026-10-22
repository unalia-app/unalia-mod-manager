"use client";

import { Button, cn } from "@modalia/ui";
import { Bug, CircleCheckBig, ChevronRight, Lightbulb, MessageCircle } from "lucide-react";
import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { toast } from "@/components/toaster";
import {
	type FeedbackType,
	MESSAGE_MAX,
	MESSAGE_MIN,
	closeFeedback,
	submitFeedback,
	useFeedbackOpen,
} from "@/lib/feedback";

const TYPES: { value: FeedbackType; label: string; icon: ReactNode }[] = [
	{ value: "bug", label: "Bug report", icon: <Bug className="size-3.5" /> },
	{ value: "feature", label: "Feature request", icon: <Lightbulb className="size-3.5" /> },
	{ value: "general", label: "General feedback", icon: <MessageCircle className="size-3.5" /> },
];

/**
 * Fenêtre Help & Feedback, reprise d'Unalia : type de retour, message, puis
 * remerciement. Dans un portail : `fixed` ne doit dépendre d'aucun parent
 * transformé (DESIGN.md).
 */
export function FeedbackDialog() {
	const open = useFeedbackOpen();
	const [type, setType] = useState<FeedbackType>("general");
	const [message, setMessage] = useState("");
	const [error, setError] = useState<string | null>(null);
	const [sending, setSending] = useState(false);
	const [sent, setSent] = useState(false);

	// Échap ferme ; la page derrière ne défile pas.
	useEffect(() => {
		if (!open) return;
		const onKey = (e: KeyboardEvent) => e.key === "Escape" && closeFeedback();
		window.addEventListener("keydown", onKey);
		const overflow = document.documentElement.style.overflow;
		document.documentElement.style.overflow = "hidden";
		return () => {
			window.removeEventListener("keydown", onKey);
			document.documentElement.style.overflow = overflow;
		};
	}, [open]);

	// Remis à zéro après la fermeture (le temps du fondu).
	useEffect(() => {
		if (open) return;
		const timer = window.setTimeout(() => {
			setSent(false);
			setType("general");
			setMessage("");
			setError(null);
		}, 300);
		return () => window.clearTimeout(timer);
	}, [open]);

	if (!open) return null;

	const label = TYPES.find((t) => t.value === type)?.label ?? "Feedback";

	const submit = async (event: FormEvent) => {
		event.preventDefault();
		if (message.trim().length < MESSAGE_MIN) {
			setError(`Message must be at least ${MESSAGE_MIN} characters`);
			return;
		}
		setSending(true);
		try {
			await submitFeedback({ type, message: message.trim() });
			setSent(true);
		} catch {
			toast({ title: "An error occurred. Please try again.", tone: "error" });
		} finally {
			setSending(false);
		}
	};

	return createPortal(
		<div className="fixed inset-0 z-[70] flex items-center justify-center p-4">
			<button
				type="button"
				aria-label="Close"
				onClick={closeFeedback}
				className="absolute inset-0 animate-fade-in cursor-default bg-black/60 backdrop-blur-sm"
			/>
			<div
				role="dialog"
				aria-modal="true"
				aria-label="Help & Feedback"
				className="relative w-full max-w-[520px] animate-fade-up overflow-hidden rounded-xl border border-white/8 bg-bg shadow-2xl shadow-black/60"
			>
				{sent ? (
					<div className="flex flex-col items-center gap-4 px-6 py-10 text-center">
						<div className="flex size-14 items-center justify-center rounded-full bg-accent/10">
							<CircleCheckBig className="size-7 text-accent" />
						</div>
						<div className="space-y-1.5">
							<h3 className="text-lg font-semibold text-fg">Thanks for your feedback!</h3>
							<p className="text-sm text-fg-subtle">We read every submission and use it to improve Unalia.</p>
						</div>
						<Button className="mt-2" onClick={closeFeedback}>
							Close
						</Button>
					</div>
				) : (
					<form onSubmit={submit}>
						{/* Fil d'Ariane */}
						<div className="flex items-center gap-1.5 border-b border-white/5 px-5 py-3">
							<MessageCircle className="size-3.5 text-fg-faint" />
							<span className="text-xs font-medium text-fg-faint">Help & Feedback</span>
							<ChevronRight className="size-3 text-white/20" />
							<span className="text-xs font-medium text-fg-subtle">{label}</span>
						</div>

						<div className="space-y-4 px-5 py-5">
							<div className="flex flex-wrap items-center gap-1.5">
								{TYPES.map((t) => (
									<button
										key={t.value}
										type="button"
										disabled={sending}
										onClick={() => setType(t.value)}
										className={cn(
											"flex cursor-pointer items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs font-medium transition-colors",
											type === t.value
												? "border-accent/35 bg-accent/12 text-accent"
												: "border-white/10 bg-white/4 text-fg-subtle hover:border-white/18 hover:text-fg-muted",
										)}
									>
										{t.icon}
										{t.label}
									</button>
								))}
							</div>

							<div className="space-y-1">
								<textarea
									value={message}
									onChange={(e) => {
										setMessage(e.target.value);
										setError(null);
									}}
									placeholder="Describe your feedback in detail..."
									maxLength={MESSAGE_MAX}
									disabled={sending}
									rows={5}
									// La fenêtre s'ouvre pour écrire : le curseur y est déjà.
									autoFocus
									className="w-full resize-none bg-transparent text-sm text-fg/80 outline-none placeholder:text-white/22"
								/>
								{error && <p className="text-xs text-danger">{error}</p>}
							</div>
						</div>

						<div className="flex items-center justify-between border-t border-white/5 px-4 py-3">
							<span className="font-tight text-xs text-fg-faint tabular-nums">
								{message.length}/{MESSAGE_MAX}
							</span>
							<div className="flex items-center gap-2">
								<Button type="button" variant="secondary" size="sm" disabled={sending} onClick={closeFeedback}>
									Cancel
								</Button>
								<Button type="submit" size="sm" disabled={sending}>
									{sending ? "Sending…" : "Send feedback"}
								</Button>
							</div>
						</div>
					</form>
				)}
			</div>
		</div>,
		document.body,
	);
}
