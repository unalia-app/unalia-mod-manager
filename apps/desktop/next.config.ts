import type { NextConfig } from "next";

// Export statique servi par Tauri : pas de serveur, donc pas de server
// actions, routes API, middleware ni optimisation d'images à la volée.
const nextConfig: NextConfig = {
  output: "export",
  images: { unoptimized: true },
  // `/game/` → `game/index.html` : servi tel quel par Tauri.
  trailingSlash: true,
  transpilePackages: ["@modalia/ui"],
  devIndicators: false,
};

export default nextConfig;
