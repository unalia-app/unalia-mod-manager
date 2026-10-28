import type { NextConfig } from "next";

// Static export served by Tauri: no server, so no server actions, API
// routes, middleware or on-the-fly image optimization.
const nextConfig: NextConfig = {
  output: "export",
  images: { unoptimized: true },
  // `/game/` gives `game/index.html`: served as is by Tauri.
  trailingSlash: true,
  transpilePackages: ["@modalia/ui"],
  devIndicators: false,
  // No AGENTS.md regenerated at each `next dev`.
  agentRules: false,
};

export default nextConfig;
