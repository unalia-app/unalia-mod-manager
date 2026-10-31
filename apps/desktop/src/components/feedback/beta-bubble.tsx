"use client";

import { Button } from "@modalia/ui";
import { MessageSquarePlus } from "lucide-react";
import { useEffect } from "react";

import { Bubble, bubbleDismissed, useBubble } from "@/components/feedback/bubble";
import { feedbackSent, openFeedback } from "@/lib/feedback";

const DISMISSED = "modalia.betaFeedbackDismissed";
const SHOW_AFTER_MS = 10_000;

let onScreen = false;

/** One bubble at a time: the others wait for this one. */
export function betaBubbleOnScreen(): boolean {
	return onScreen;
}

/**
 * Unalia's "beta" bubble: 10 s after opening, bottom right, an invitation
 * to give feedback. Closed or used, it never comes back.
 */
export function BetaBubble() {
	const { visible, leaving, show, dismiss } = useBubble(DISMISSED);

	useEffect(() => {
		if (bubbleDismissed(DISMISSED) || feedbackSent()) return;
		const timer = window.setTimeout(show, SHOW_AFTER_MS);
		return () => window.clearTimeout(timer);
	}, [show]);

	useEffect(() => {
		onScreen = visible;
		return () => {
			onScreen = false;
		};
	}, [visible]);

	if (!visible) return null;

	return (
		<Bubble leaving={leaving} onDismiss={dismiss}>
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
		</Bubble>
	);
}
