"use client";

import { Button, cn } from "@modalia/ui";
import { MessageSquarePlus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { feedbackSent, openFeedback } from "@/lib/feedback";

const DISMISSED = "modalia.betaFeedbackDismissed";
const SHOW_AFTER_MS = 10_000;

/**
 * Bulle « beta » d'Unalia : 10 s après l'ouverture, en bas à droite, une
 * invitation à donner son avis. Fermée ou utilisée, elle ne revient plus.
 */
export function BetaBubble() {
	const [visible, setVisible] = useState(false);
	const [leaving, setLeaving] = useState(false);

	useEffect(() => {
		let dismissed = false;
		try {
			dismissed = localStorage.getItem(DISMISSED) !== null;
		} catch {
			// Stockage bloqué : on la montre, une fois par lancement.
		}
		if (dismissed || feedbackSent()) return;
		const timer = window.setTimeout(() => setVisible(true), SHOW_AFTER_MS);
		return () => window.clearTimeout(timer);
	}, []);

	if (!visible) return null;

	const dismiss = () => {
		setLeaving(true);
		window.setTimeout(() => setVisible(false), 200);
		try {
			localStorage.setItem(DISMISSED, "1");
		} catch {
			// Sans stockage, elle reviendra au prochain lancement.
		}
	};

	return createPortal(
		<div
			className={cn(
				"fixed right-6 bottom-6 z-50 w-[260px] transition-[opacity,translate] duration-200 ease-out",
				leaving ? "translate-y-4 opacity-0" : "animate-fade-up",
			)}
		>
			<div className="relative overflow-hidden rounded-xl border border-white/10 bg-bg shadow-xl shadow-black/40">
				{/* Liseré en haut, comme dans Unalia */}
				<div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-accent/60 to-transparent" />
				<button
					type="button"
					onClick={dismiss}
					aria-label="Dismiss"
					className="absolute top-2.5 right-2.5 cursor-pointer rounded-md p-1 text-white/30 transition-colors hover:bg-white/10 hover:text-white/70"
				>
					<X className="size-3.5" />
				</button>
				<div className="p-4">
					<span className="mb-2.5 inline-flex items-center rounded-md border border-accent/30 bg-accent/10 px-2 py-0.5 text-[10px] font-semibold tracking-wide text-accent uppercase">
						Beta
					</span>
					<p className="text-sm leading-snug font-semibold text-fg">You're part of the beta!</p>
					<p className="mt-1 text-xs leading-relaxed text-fg-subtle">
						Your feedback helps shape Unalia. Tell us what you think.
					</p>
					<Button
						className="mt-3 w-full"
						leftIcon={<MessageSquarePlus className="size-3.5" />}
						onClick={() => {
							openFeedback();
							dismiss();
						}}
					>
						Share feedback
					</Button>
				</div>
			</div>
		</div>,
		document.body,
	);
}
