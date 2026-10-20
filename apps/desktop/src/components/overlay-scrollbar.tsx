"use client";

import { cn } from "@modalia/ui";
import { type PointerEvent, useCallback, useEffect, useRef, useState } from "react";

/** Délai après le dernier défilement avant d'effacer la barre. */
const HIDE_AFTER = 1000;
/** Hauteur de l'en-tête collant : la barre commence dessous. */
const TOP = 56;
const MIN_THUMB = 32;

/**
 * Barre de défilement de la page, en surimpression : elle ne prend aucune
 * place (la barre native est masquée, voir `styles.css`), n'apparaît que
 * pendant le défilement ou au survol du bord droit, et se laisse attraper.
 */
export function OverlayScrollbar() {
	const [metrics, setMetrics] = useState({ size: 0, offset: 0, scrollable: false });
	const [active, setActive] = useState(false);
	const [hovered, setHovered] = useState(false);
	const [dragging, setDragging] = useState(false);
	const hideTimer = useRef<number | undefined>(undefined);
	const drag = useRef({ startY: 0, startScroll: 0 });

	const measure = useCallback(() => {
		const doc = document.documentElement;
		const track = window.innerHeight - TOP;
		const total = doc.scrollHeight;
		const visible = window.innerHeight;
		if (total <= visible + 1) {
			setMetrics({ size: 0, offset: 0, scrollable: false });
			return;
		}
		const size = Math.max((visible / total) * track, MIN_THUMB);
		const offset = (window.scrollY / (total - visible)) * (track - size);
		setMetrics({ size, offset, scrollable: true });
	}, []);

	useEffect(() => {
		const onScroll = () => {
			measure();
			setActive(true);
			window.clearTimeout(hideTimer.current);
			hideTimer.current = window.setTimeout(() => setActive(false), HIDE_AFTER);
		};
		measure();
		window.addEventListener("scroll", onScroll, { passive: true });
		window.addEventListener("resize", measure);
		// La hauteur change sans défilement (chargement de cartes, onglets).
		const observer = new ResizeObserver(measure);
		observer.observe(document.body);
		return () => {
			window.removeEventListener("scroll", onScroll);
			window.removeEventListener("resize", measure);
			observer.disconnect();
			window.clearTimeout(hideTimer.current);
		};
	}, [measure]);

	const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
		e.preventDefault();
		e.currentTarget.setPointerCapture(e.pointerId);
		drag.current = { startY: e.clientY, startScroll: window.scrollY };
		setDragging(true);
	};

	const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
		if (!dragging) return;
		const doc = document.documentElement;
		const track = window.innerHeight - TOP - metrics.size;
		const range = doc.scrollHeight - window.innerHeight;
		const delta = ((e.clientY - drag.current.startY) / Math.max(track, 1)) * range;
		window.scrollTo({ top: drag.current.startScroll + delta });
	};

	if (!metrics.scrollable) return null;
	const shown = active || hovered || dragging;

	return (
		<div
			aria-hidden
			onPointerEnter={() => setHovered(true)}
			onPointerLeave={() => setHovered(false)}
			className="fixed right-0 bottom-0 z-40 w-3"
			style={{ top: TOP }}
		>
			<div
				onPointerDown={onPointerDown}
				onPointerMove={onPointerMove}
				onPointerUp={() => setDragging(false)}
				onPointerCancel={() => setDragging(false)}
				style={{ height: metrics.size, transform: `translateY(${metrics.offset}px)` }}
				className={cn(
					"absolute right-[3px] rounded-full transition-[opacity,width,background-color] duration-200 ease-out",
					shown ? "opacity-100" : "opacity-0",
					hovered || dragging ? "w-2 bg-white/35" : "w-1.5 bg-white/22",
				)}
			/>
		</div>
	);
}
