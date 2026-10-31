"use client";

import { cn } from "@modalia/ui";
import { type CSSProperties, useEffect, useRef, useState } from "react";

/** Pixels per second, whatever the length: the more is cut, the longer it takes. */
const SPEED = 50;
/** Share of the cycle spent sliding one way (see `marquee` in styles.css). */
const SLIDE = 0.4;
const MIN_CYCLE = 3;

/**
 * One line of text, cut if it doesn't fit: on hover it slides to its end and
 * back for as long as the pointer stays, like on Discord.
 */
export function Marquee({
	text,
	className,
	hoverOn,
}: {
	text: string;
	className?: string;
	/** Selector of the ancestor to watch, when something covers the text (a card's stretched link). */
	hoverOn?: string;
}) {
	const inner = useRef<HTMLSpanElement>(null);
	const [cut, setCut] = useState(0);

	useEffect(() => {
		const el = inner.current;
		const area = hoverOn ? el?.closest(hoverOn) : el?.parentElement;
		if (!el || !area) return;
		const measure = () => setCut(Math.max(el.scrollWidth - el.clientWidth, 0));
		const stop = () => setCut(0);
		area.addEventListener("mouseenter", measure);
		area.addEventListener("mouseleave", stop);
		return () => {
			area.removeEventListener("mouseenter", measure);
			area.removeEventListener("mouseleave", stop);
		};
	}, [hoverOn]);

	const motion = {
		"--marquee-shift": `${-cut}px`,
		animation: `marquee ${Math.max(cut / SPEED / SLIDE, MIN_CYCLE)}s linear infinite`,
	} as CSSProperties;

	return (
		<span title={text} className={cn("block min-w-0 overflow-hidden", className)}>
			<span
				ref={inner}
				style={cut > 0 ? motion : undefined}
				className={cut > 0 ? "inline-block whitespace-nowrap" : "block truncate"}
			>
				{text}
			</span>
		</span>
	);
}
