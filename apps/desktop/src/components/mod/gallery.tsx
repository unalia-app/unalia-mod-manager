"use client";

import { cn } from "@modalia/ui";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";

/**
 * Images of a mod: the sources publish no gallery (Nexus only gives its main
 * picture), so it is the main picture plus the images of the description,
 * where authors put their screenshots. Badges, buttons and icons are left out
 * by their size once loaded.
 */

/** Smaller than this, it's a badge, a button or an icon, not a screenshot. */
const MIN_WIDTH = 320;
const MIN_HEIGHT = 180;
const MAX_IMAGES = 24;

/** HTTPS images of a description (Markdown and HTML), in order, without repeats. */
export function descriptionImages(markdown: string): string[] {
	const found = [
		...markdown.matchAll(/!\[[^\]]*\]\(\s*<?(https:\/\/[^)\s>]+)/g),
		...markdown.matchAll(/<img\b[^>]*\bsrc\s*=\s*["'](https:\/\/[^"']+)["']/gi),
	]
		.sort((a, b) => (a.index ?? 0) - (b.index ?? 0))
		.map((m) => m[1])
		.filter((src): src is string => Boolean(src) && !/\.svg(\?|$)/i.test(src as string));
	return [...new Set(found)].slice(0, MAX_IMAGES);
}

/**
 * The candidate images that turn out to be screenshots once loaded, in
 * order. Loading happens in `GalleryStrip`, which reports each size here.
 */
export function useGallery(candidates: string[]) {
	const [sizes, setSizes] = useState<Record<string, boolean>>({});
	const images = useMemo(() => candidates.filter((src) => sizes[src]), [candidates, sizes]);
	const measured = useCallback(
		(src: string, large: boolean) => setSizes((s) => (s[src] === large ? s : { ...s, [src]: large })),
		[],
	);
	return { images, measured };
}

/**
 * Row of thumbnails. Every candidate loads hidden: only the large ones show.
 * Nothing at all with fewer than two screenshots, the header already shows
 * the main picture. The scrollbars are hidden in the app and a mouse wheel
 * scrolls the page: arrows at each end when there is more to see.
 */
export function GalleryStrip({
	candidates,
	images,
	measured,
	onOpen,
}: {
	candidates: string[];
	images: string[];
	measured: (src: string, large: boolean) => void;
	onOpen: (index: number) => void;
}) {
	const shown = images.length >= 2;
	const row = useRef<HTMLDivElement>(null);
	const [ends, setEnds] = useState({ left: false, right: false });

	const measure = useCallback(() => {
		const el = row.current;
		if (!el) return;
		const left = el.scrollLeft > 4;
		const right = el.scrollLeft + el.clientWidth < el.scrollWidth - 4;
		setEnds((e) => (e.left === left && e.right === right ? e : { left, right }));
	}, []);

	// Again when a thumbnail shows up (`images` changes the width of the row)
	// and when the window is resized.
	useEffect(() => {
		measure();
		window.addEventListener("resize", measure);
		return () => window.removeEventListener("resize", measure);
	}, [measure, images]);

	const scroll = (direction: number) => {
		const el = row.current;
		el?.scrollBy({ left: direction * el.clientWidth * 0.8, behavior: "smooth" });
	};

	return (
		<div className={cn("relative mt-6", !shown && "hidden")}>
			<div ref={row} onScroll={measure} className="-mx-1 flex gap-3 overflow-x-auto px-1">
				{candidates.map((src) => {
					const index = images.indexOf(src);
					return (
						<button
							key={src}
							type="button"
							onClick={() => onOpen(index)}
							aria-label={`Image ${index + 1} of ${images.length}`}
							className={cn(
								"aspect-video h-28 shrink-0 cursor-zoom-in overflow-hidden rounded-lg border border-line bg-white/[0.04] transition hover:border-white/25 hover:brightness-110",
								index < 0 && "hidden",
							)}
						>
							<img
								src={src}
								alt=""
								decoding="async"
								referrerPolicy="no-referrer"
								onLoad={(e) =>
									measured(
										src,
										e.currentTarget.naturalWidth >= MIN_WIDTH && e.currentTarget.naturalHeight >= MIN_HEIGHT,
									)
								}
								onError={() => measured(src, false)}
								className="size-full object-cover"
							/>
						</button>
					);
				})}
			</div>
			{ends.left && (
				<div className="pointer-events-none absolute inset-y-0 left-0 flex w-20 items-center bg-gradient-to-r from-bg to-transparent">
					<button
						type="button"
						aria-label="Previous images"
						onClick={() => scroll(-1)}
						className={cn(ROUND, "pointer-events-auto relative size-9")}
					>
						<ChevronLeft className="size-5" />
					</button>
				</div>
			)}
			{ends.right && (
				<div className="pointer-events-none absolute inset-y-0 right-0 flex w-20 items-center justify-end bg-gradient-to-l from-bg to-transparent">
					<button
						type="button"
						aria-label="Next images"
						onClick={() => scroll(1)}
						className={cn(ROUND, "pointer-events-auto relative size-9")}
					>
						<ChevronRight className="size-5" />
					</button>
				</div>
			)}
		</div>
	);
}

/** Full-screen viewer: arrows or ← →, Escape or a click outside closes. */
export function ImageViewer({
	images,
	index,
	onIndex,
	onClose,
}: {
	images: string[];
	index: number;
	onIndex: (index: number) => void;
	onClose: () => void;
}) {
	const count = images.length;
	const go = useCallback((step: number) => onIndex((index + step + count) % count), [index, count, onIndex]);

	// Keyboard; the page behind doesn't scroll.
	useEffect(() => {
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") onClose();
			else if (e.key === "ArrowLeft" && count > 1) go(-1);
			else if (e.key === "ArrowRight" && count > 1) go(1);
		};
		window.addEventListener("keydown", onKey);
		const overflow = document.documentElement.style.overflow;
		document.documentElement.style.overflow = "hidden";
		return () => {
			window.removeEventListener("keydown", onKey);
			document.documentElement.style.overflow = overflow;
		};
	}, [go, onClose, count]);

	const src = images[index];
	if (!src) return null;

	return createPortal(
		<div role="dialog" aria-modal="true" aria-label="Images" className="fixed inset-0 z-[70] flex items-center justify-center">
			<button
				type="button"
				aria-label="Close"
				onClick={onClose}
				className="absolute inset-0 animate-fade-in cursor-default bg-black/85 backdrop-blur-sm"
			/>
			<img
				key={src}
				src={src}
				alt=""
				referrerPolicy="no-referrer"
				className="relative max-h-[86vh] max-w-[86vw] animate-fade-in rounded-lg object-contain shadow-2xl shadow-black/60"
			/>
			<button
				type="button"
				aria-label="Close"
				onClick={onClose}
				className={cn(ROUND, "top-5 right-5")}
			>
				<X className="size-5" />
			</button>
			{count > 1 && (
				<>
					<button type="button" aria-label="Previous image" onClick={() => go(-1)} className={cn(ROUND, "left-5")}>
						<ChevronLeft className="size-6" />
					</button>
					<button type="button" aria-label="Next image" onClick={() => go(1)} className={cn(ROUND, "right-5")}>
						<ChevronRight className="size-6" />
					</button>
					<span className="absolute bottom-5 rounded-full bg-black/50 px-3 py-1 font-tight text-xs text-white/80 tabular-nums">
						{index + 1} / {count}
					</span>
				</>
			)}
		</div>,
		document.body,
	);
}

const ROUND =
	"absolute flex size-11 cursor-pointer items-center justify-center rounded-full bg-black/50 text-white/80 transition-colors hover:bg-black/70 hover:text-white";
