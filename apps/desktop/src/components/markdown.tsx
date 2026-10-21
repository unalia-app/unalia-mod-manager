"use client";

import { useRouter } from "next/navigation";
import { type ComponentProps, useMemo } from "react";
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

function img({ src, alt }: ComponentProps<"img">) {
	return typeof src === "string" && src.startsWith("https://") ? (
		<img src={src} alt={alt ?? ""} loading="lazy" decoding="async" referrerPolicy="no-referrer" />
	) : null;
}

/**
 * `internalLink` : page de l'appli pour un lien de la description (un autre
 * mod du même jeu), sinon `null` et le lien s'ouvre dans le navigateur.
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
					// Ancres et liens relatifs : ils mèneraient dans l'appli elle-même.
					return <span>{label}</span>;
				}
				const inside = internalLink?.(href) ?? null;
				return (
					<a
						href={href}
						title={inside ? "Open in Unalia" : undefined}
						onClick={(event) => {
							// Jamais de navigation vers un site dans la fenêtre de l'appli.
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
