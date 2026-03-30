import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./app/**/*.{ts,tsx}",
    "./lib/**/*.{ts,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        canvas: "#0C0C0C",
        surface: "#111111",
        "surface-hover": "#1A1A1A",
        border: "#333333",
        "border-subtle": "#222222",
        "text-primary": "#E0E0E0",
        "text-muted": "#888888",
        "text-dim": "#555555",
        accent: "#B4F0A0",
        "accent-dim": "rgba(180, 240, 160, 0.10)",
        success: "#B4F0A0",
        warning: "#F0D080",
        error: "#F09090",
      },
      fontFamily: {
        mono: ["'Berkeley Mono', 'JetBrains Mono', 'Geist Mono', 'SF Mono', monospace"],
      },
      fontSize: {
        "label": ["11px", { lineHeight: "16px", letterSpacing: "0.05em" }],
        "small": ["13px", { lineHeight: "20px" }],
        "body": ["14px", { lineHeight: "22px" }],
        "body-lg": ["16px", { lineHeight: "26px" }],
        "section": ["18px", { lineHeight: "28px" }],
        "title": ["24px", { lineHeight: "32px" }],
        "hero": ["40px", { lineHeight: "48px" }],
      },
      spacing: {
        "2xs": "2px",
        "xs": "4px",
        "sm": "8px",
        "md": "16px",
        "lg": "24px",
        "xl": "32px",
        "2xl": "48px",
        "3xl": "64px",
      },
      maxWidth: {
        "receipt": "560px",
        "content": "640px",
        "leaderboard": "800px",
      },
    },
  },
  plugins: [],
};

export default config;
