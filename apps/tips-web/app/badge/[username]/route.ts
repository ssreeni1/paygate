import { NextRequest, NextResponse } from "next/server";
import { getTipsByRecipient, formatUsdc } from "@/lib/api";

export async function GET(
  _request: NextRequest,
  { params }: { params: { username: string } }
) {
  const { username } = params;

  let totalUsdc = 0;
  let agentCount = 0;

  try {
    const tips = await getTipsByRecipient(username);
    totalUsdc = tips.reduce((sum, t) => sum + t.amount_usdc, 0);
    const uniqueAgents = new Set(tips.map((t) => t.sender_name));
    agentCount = uniqueAgents.size;
  } catch {
    // Return a badge with zero values on error
  }

  const amountText = formatUsdc(totalUsdc);
  const valueText = `${amountText} from ${agentCount} agent${agentCount !== 1 ? "s" : ""}`;

  const labelWidth = 80;
  const valueWidth = Math.max(120, valueText.length * 7.5 + 20);
  const totalWidth = labelWidth + valueWidth;
  const height = 20;

  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${totalWidth}" height="${height}" role="img" aria-label="agent tips: ${valueText}">
  <title>agent tips: ${valueText}</title>
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#141416" stop-opacity=".3"/>
    <stop offset="1" stop-opacity=".15"/>
  </linearGradient>
  <clipPath id="r">
    <rect width="${totalWidth}" height="${height}" rx="3" fill="#fff"/>
  </clipPath>
  <g clip-path="url(#r)">
    <rect width="${labelWidth}" height="${height}" fill="#141416"/>
    <rect x="${labelWidth}" width="${valueWidth}" height="${height}" fill="#0A0A0B"/>
    <rect width="${totalWidth}" height="${height}" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Geist Mono,DejaVu Sans Mono,Verdana,monospace" text-rendering="geometricPrecision" font-size="11">
    <text x="${labelWidth / 2}" y="14" fill="#A1A1AA">agent tips</text>
    <text x="${labelWidth + valueWidth / 2}" y="14" fill="#22D3EE">${valueText}</text>
  </g>
</svg>`;

  return new NextResponse(svg, {
    status: 200,
    headers: {
      "Content-Type": "image/svg+xml",
      "Cache-Control": "public, max-age=300",
    },
  });
}
