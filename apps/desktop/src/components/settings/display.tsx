"use client";

import { Segmented } from "@modalia/ui";

import { type InterfaceSize, SIZES, autoScale, setInterfaceSize, useInterfaceSize } from "@/lib/display";

const percent = (scale: number) => `${Math.round(scale * 100)}%`;

/** Taille de l'interface : automatique selon l'écran, ou fixée. */
export function Display() {
	const size = useInterfaceSize();
	const items = SIZES.map((s) => ({
		value: String(s),
		label: s === "auto" ? `Auto (${percent(autoScale())})` : percent(s),
	}));

	return (
		<>
			<div className="space-y-1">
				<h1 className="text-2xl font-semibold tracking-tight">Display</h1>
				<p className="text-sm text-fg-subtle">Make Unalia larger on big, high-resolution screens.</p>
			</div>

			<div className="rounded-xl border border-line bg-white/[0.03] p-5 sm:p-6">
				<h3 className="text-sm font-semibold text-fg">Interface size</h3>
				<p className="mt-0.5 text-xs text-fg-subtle">
					Text, spacing and icons, all at once. Auto picks a size from your screen's resolution: larger on 2K and 4K
					screens.
				</p>
				<div className="mt-4">
					<Segmented
						label="Interface size"
						items={items}
						value={String(size)}
						onChange={(value) => setInterfaceSize((value === "auto" ? "auto" : Number(value)) as InterfaceSize)}
					/>
				</div>
			</div>
		</>
	);
}
