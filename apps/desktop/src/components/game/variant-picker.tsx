"use client";

import { Button, cn } from "@modalia/ui";
import { Layers } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { settleVariantChoice, useVariantChoice } from "@/lib/installs";

/**
 * Variant picker for a mod whose download holds the same files several
 * times, one folder per variant (`Skip Intro`, `Skip Intro + Instant
 * Menu`). Only one can go into the game; updates keep it. In a portal:
 * `fixed` must not depend on any transformed parent.
 */
export function VariantPicker() {
	const choice = useVariantChoice();
	const [picked, setPicked] = useState<string | null>(null);

	// New choice: the first one ticked, Install is one click away.
	useEffect(() => {
		setPicked(choice?.variants[0] ?? null);
	}, [choice]);

	useEffect(() => {
		if (!choice) return;
		const onKey = (e: KeyboardEvent) => e.key === "Escape" && settleVariantChoice(null);
		window.addEventListener("keydown", onKey);
		const overflow = document.documentElement.style.overflow;
		document.documentElement.style.overflow = "hidden";
		return () => {
			window.removeEventListener("keydown", onKey);
			document.documentElement.style.overflow = overflow;
		};
	}, [choice]);

	if (!choice) return null;

	return createPortal(
		<div className="fixed inset-0 z-[70] flex items-center justify-center p-4">
			<button
				type="button"
				aria-label="Close"
				onClick={() => settleVariantChoice(null)}
				className="absolute inset-0 animate-fade-in cursor-default bg-black/60 backdrop-blur-sm"
			/>
			<div
				role="dialog"
				aria-modal="true"
				aria-label="Choose a variant"
				className="relative flex max-h-[85vh] w-full max-w-[480px] animate-fade-up flex-col overflow-hidden rounded-xl border border-white/8 bg-bg shadow-2xl shadow-black/60"
			>
				<div className="flex items-center gap-1.5 border-b border-white/5 px-5 py-3">
					<Layers className="size-3.5 text-fg-faint" />
					<span className="truncate text-xs font-medium text-fg-subtle">{choice.name ?? "This mod"}</span>
				</div>

				<div className="px-5 pt-5">
					<h3 className="text-base font-semibold text-fg">Choose a variant</h3>
					<p className="mt-1 text-xs leading-relaxed text-fg-subtle">
						This download holds the mod in several variants, and only one can go into the game. Updates keep the one
						you pick.
					</p>
				</div>

				<ul role="radiogroup" className="mt-4 min-h-0 flex-1 space-y-1.5 overflow-y-auto px-5 pb-1">
					{choice.variants.map((variant) => {
						const checked = picked === variant;
						return (
							<li key={variant}>
								<button
									type="button"
									role="radio"
									aria-checked={checked}
									onClick={() => setPicked(variant)}
									className={cn(
										"flex w-full cursor-pointer items-center gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors",
										checked ? "border-accent/35 bg-accent/8" : "border-line bg-surface hover:border-line-strong",
									)}
								>
									<span
										className={cn(
											"flex size-4 shrink-0 items-center justify-center rounded-full border",
											checked ? "border-accent" : "border-line-strong",
										)}
									>
										{checked && <span className="size-2 rounded-full bg-accent" />}
									</span>
									<span className="min-w-0 flex-1 truncate text-sm font-medium text-fg">{variant}</span>
								</button>
							</li>
						);
					})}
				</ul>

				<div className="mt-4 flex items-center justify-end gap-2 border-t border-white/5 px-4 py-3">
					<Button type="button" variant="secondary" size="sm" onClick={() => settleVariantChoice(null)}>
						Cancel
					</Button>
					<Button type="button" size="sm" disabled={!picked} onClick={() => settleVariantChoice(picked)}>
						Install
					</Button>
				</div>
			</div>
		</div>,
		document.body,
	);
}
