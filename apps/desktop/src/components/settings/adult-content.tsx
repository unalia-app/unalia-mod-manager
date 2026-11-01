"use client";

import { RangeSlider, Switch } from "@modalia/ui";
import type { ReactNode } from "react";

import { BLUR_MAX, BLUR_MIN, BLUR_STEP, blurStyle, setAdultContent, useAdultContent } from "@/lib/adult-content";

/** Adult mods: how much their images are blurred. */
export function AdultContent() {
	const { blur, strength } = useAdultContent();

	return (
		<>
			<div className="space-y-1">
				<h1 className="text-2xl font-semibold tracking-tight">Adult content</h1>
				<p className="text-sm text-fg-subtle">Mods their authors mark as adult content. For players 18 and over.</p>
			</div>

			<div className="rounded-xl border border-line bg-white/[0.03] p-5 sm:p-6">
				<Row title="Blur their images" details="On cards, in My Mods and on their pages. Click an image on a mod's page to see it as is.">
					<Switch label="Blur their images" checked={blur} onChange={(checked) => setAdultContent({ blur: checked })} />
				</Row>
				<div className="mt-5 flex items-center gap-5">
					<div className="min-w-0 flex-1">
						<h3 className="text-sm font-semibold text-fg">Blur strength</h3>
						<RangeSlider
							className="mt-3"
							aria-label="Blur strength"
							value={strength}
							min={BLUR_MIN}
							max={BLUR_MAX}
							step={BLUR_STEP}
							disabled={!blur}
							formatValueText={(value) => `${value} pixels`}
							onValueChange={(value) => setAdultContent({ strength: value })}
						/>
					</div>
					{/* What the slider does, on something safe to look at. */}
					<div aria-hidden className="h-20 w-36 shrink-0 overflow-hidden rounded-lg border border-line bg-bg">
						<div
							style={blur ? blurStyle(strength) : undefined}
							className="flex size-full items-center justify-center bg-gradient-to-br from-accent/70 to-info/40 text-lg font-semibold text-fg"
						>
							Unalia
						</div>
					</div>
				</div>
			</div>
		</>
	);
}

function Row({ title, details, children }: { title: string; details: string; children: ReactNode }) {
	return (
		<div className="flex items-center justify-between gap-6">
			<div className="min-w-0">
				<h3 className="text-sm font-semibold text-fg">{title}</h3>
				<p className="mt-0.5 text-xs text-fg-subtle">{details}</p>
			</div>
			{children}
		</div>
	);
}
