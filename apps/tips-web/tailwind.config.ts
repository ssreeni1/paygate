import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./app/**/*.{ts,tsx}",
    "./lib/**/*.{ts,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        canvas: "#0A0A0B",
        surface: "#141416",
        "surface-hover": "#1C1C1F",
        border: "#27272A",
        "border-subtle": "#1E1E21",
        "text-primary": "#FAFAFA",
        "text-muted": "#A1A1AA",
        "text-dim": "#71717A",
        accent: "#22D3EE",
        "accent-dim": "rgba(34, 211, 238, 0.15)",
        "accent-glow": "rgba(34, 211, 238, 0.3)",
        success: "#4ADE80",
        warning: "#FBBF24",
        error: "#F87171",
        info: "#38BDF8",
      },
      fontFamily: {
        display: ["Satoshi", "DM Sans", "sans-serif"],
        body: ["DM Sans", "sans-serif"],
        mono: ["Geist Mono", "monospace"],
      },
      fontSize: {
        "label": ["11px", { lineHeight: "16px" }],
        "small": ["13px", { lineHeight: "20px" }],
        "body": ["14px", { lineHeight: "22px" }],
        "body-lg": ["16px", { lineHeight: "24px" }],
        "subtitle": ["18px", { lineHeight: "28px" }],
        "section": ["22px", { lineHeight: "30px" }],
        "title": ["32px", { lineHeight: "40px" }],
        "display": ["48px", { lineHeight: "56px" }],
        "hero": ["56px", { lineHeight: "64px" }],
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
      borderRadius: {
        "sm": "4px",
        "md": "8px",
        "lg": "12px",
        "full": "9999px",
      },
      maxWidth: {
        "receipt": "520px",
        "content": "640px",
        "leaderboard": "1100px",
      },
      boxShadow: {
        "cyan-glow": "0 0 0 3px rgba(34, 211, 238, 0.15), 0 0 20px rgba(34, 211, 238, 0.3)",
      },
      keyframes: {
        "count-up": {
          "0%": { opacity: "0", transform: "translateY(8px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        "fade-in": {
          "0%": { opacity: "0" },
          "100%": { opacity: "1" },
        },
      },
      animation: {
        "count-up": "count-up 300ms ease-out",
        "fade-in": "fade-in 150ms ease-out",
      },
    },
  },
  plugins: [],
};

export default config;
