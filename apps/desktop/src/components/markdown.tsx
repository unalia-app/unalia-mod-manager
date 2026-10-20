"use client";

import type { ComponentProps } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeRaw from "rehype-raw";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import remarkGfm from "remark-gfm";

import { openExternal } from "@/lib/open";

/**
 * Le HTML des descriptions n'est jamais rendu tel quel (SPEC §7) : il est
 * analysé (rehype-raw) puis passé dans une liste blanche (rehype-sanitize,
 * schéma de GitHub) qui retire scripts, styles, iframes et gestionnaires
 * d'événements. Liens et images : HTTPS seulement.
 */
const schema = {
	...defaultSchema,
	protocols: { ...defaultSchema.protocols, href: ["https"], src: ["https"] },
};

const components: Components = {
	a: ({ href, children }) =>
		href?.startsWith("https://") ? (
			<a
				href={href}
				onClick={(event) => {
					// Jamais de navigation dans la fenêtre de l'appli.
					event.preventDefault();
					void openExternal(href);
				}}
			>
				{children}
			</a>
		) : (
			// Ancres et liens relatifs : ils mèneraient dans l'appli elle-même.
			<span>{children}</span>
		),
	img: ({ src, alt }: ComponentProps<"img">) =>
		typeof src === "string" && src.startsWith("https://") ? (
			<img src={src} alt={alt ?? ""} loading="lazy" decoding="async" referrerPolicy="no-referrer" />
		) : null,
};

export function Markdown({ children }: { children: string }) {
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
