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
    agentCount = new Set(tips.map((t) => t.sender_name)).size;
  } catch {}

  const amountText = formatUsdc(totalUsdc);
  const valueText = `${amountText} from ${agentCount} agent${agentCount !== 1 ? "s" : ""}`;

  const labelWidth = 80;
  const valueWidth = Math.max(120, valueText.length * 7 + 16);
  const totalWidth = labelWidth + valueWidth;

  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${totalWidth}" height="20" role="img" aria-label="agent tips: ${valueText}">
  <title>agent tips: ${valueText}</title>
  <clipPath id="r"><rect width="${totalWidth}" height="20" rx="2" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="${labelWidth}" height="20" fill="#111"/>
    <rect x="${labelWidth}" width="${valueWidth}" height="20" fill="#0C0C0C"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="monospace" font-size="11">
    <text x="${labelWidth / 2}" y="14" fill="#555">agent tips</text>
    <text x="${labelWidth + valueWidth / 2}" y="14" fill="#B4F0A0">${valueText}</text>
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
