import { ImageResponse } from "@vercel/og";
import { getTip, formatUsdc } from "@/lib/api";

export const runtime = "edge";
export const alt = "Agent Tip Receipt";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

export default async function OGImage({ params }: { params: { id: string } }) {
  let tip;
  try {
    tip = await getTip(params.id);
  } catch {
    return new ImageResponse(
      (
        <div
          style={{
            width: "100%",
            height: "100%",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            backgroundColor: "#0C0C0C",
            color: "#555",
            fontFamily: "monospace",
            fontSize: 24,
          }}
        >
          tip not found
        </div>
      ),
      { ...size }
    );
  }

  const amount = formatUsdc(tip.amount_usdc);
  const pkg = tip.package_name || tip.recipient_gh;
  const sender = tip.sender_name || "agent";
  const statusChar =
    tip.status === "paid" ? "●" :
    tip.status === "escrowed" ? "○" :
    tip.status === "claimed" ? "✓" : "×";

  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          padding: "60px 80px",
          backgroundColor: "#0C0C0C",
          fontFamily: "monospace",
          position: "relative",
        }}
      >
        {/* Top border line */}
        <div
          style={{
            position: "absolute",
            top: 0,
            left: 0,
            right: 0,
            height: 2,
            backgroundColor: "#333",
          }}
        />

        {/* Amount */}
        <div
          style={{
            fontSize: 72,
            fontWeight: 700,
            color: "#B4F0A0",
            marginBottom: 12,
            display: "flex",
          }}
        >
          {amount}
        </div>

        {/* Action line */}
        <div
          style={{
            fontSize: 28,
            color: "#888",
            marginBottom: 24,
            display: "flex",
          }}
        >
          <span style={{ color: "#E0E0E0" }}>{sender}</span>
          <span style={{ margin: "0 12px" }}>{"->"}</span>
          <span style={{ color: "#E0E0E0" }}>@{tip.recipient_gh}</span>
          {tip.package_name && (
            <span style={{ color: "#555", marginLeft: 12 }}>
              for {tip.package_name}
            </span>
          )}
        </div>

        {/* Reason */}
        {tip.reason && (
          <div
            style={{
              fontSize: 20,
              color: "#555",
              fontStyle: "italic",
              marginBottom: 40,
              display: "flex",
            }}
          >
            &ldquo;{tip.reason.length > 120 ? tip.reason.slice(0, 120) + "..." : tip.reason}&rdquo;
          </div>
        )}

        {/* Footer */}
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            position: "absolute",
            bottom: 40,
            left: 80,
            right: 80,
          }}
        >
          <div
            style={{
              fontSize: 18,
              color: tip.status === "paid" || tip.status === "claimed" ? "#B4F0A0" : "#F0D080",
              display: "flex",
            }}
          >
            {statusChar} {tip.status}
          </div>
          <div
            style={{
              fontSize: 16,
              color: "#333",
              display: "flex",
            }}
          >
            agent-tips
          </div>
        </div>

        {/* Bottom border line */}
        <div
          style={{
            position: "absolute",
            bottom: 0,
            left: 0,
            right: 0,
            height: 2,
            backgroundColor: "#333",
          }}
        />
      </div>
    ),
    { ...size }
  );
}
