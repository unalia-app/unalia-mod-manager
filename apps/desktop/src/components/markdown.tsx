"use client";

import { useRouter } from "next/navigation";
import { type ComponentProps, useMemo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeRaw from "rehype-raw";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import remarkGfm from "remark-gfm";

import { openExternal } from "@/lib/open";

/**
 * The HTML of descriptions is never rendered as is: it is parsed
 * (rehype-raw) then passed through an allowlist (rehype-sanitize, GitHub's
 * schema) that removes scripts, styles, iframes and event handlers. Links
 * and images: HTTPS only.
 */
const schema = {
	...defaultSchema,
	protocols: { ...defaultSchema.protocols, href: ["https"], src: ["https"] },
};

function img({ src, alt }: ComponentProps<"img">) {
	return typeof src === "string" && src.startsWith("https://") ? (
		<img src={src} alt={alt ?? ""} loading="lazy" decoding="async" referrerPolicy="no-referrer" />
	) : null;
}

/**
 * `internalLink`: app page for a link in the description (another mod of
 * the same game), else `null` and the link opens in the browser.
 */
export function Markdown({
	children,
	internalLink,
}: {
	children: string;
	internalLink?: (href: string) => string | null;
}) {
	const router = useRouter();
	const components: Components = useMemo(
		() => ({
			a: ({ href, children: label }) => {
				if (!href?.startsWith("https://")) {
					// Anchors and relative links: they would lead into the app itself.
					return <span>{label}</span>;
				}
				const inside = internalLink?.(href) ?? null;
				return (
					<a
						href={href}
						title={inside ? "Open in Unalia" : undefined}
						onClick={(event) => {
							// Never navigate to a website inside the app window.
							event.preventDefault();
							if (inside) router.push(inside);
							else void openExternal(href);
						}}
					>
						{label}
					</a>
				);
			},
			img,
		}),
		[internalLink, router],
	);

	return (
		<div className="prose-mod">
			<ReactMarkdown
				remarkPlugins={[remarkGfm]}
				rehypePlugins={[rehypeRaw, [rehypeSanitize, schema]]}
				components={components}
			>
				{children}
			</ReactMarkdown>
		</div>
	);
}
