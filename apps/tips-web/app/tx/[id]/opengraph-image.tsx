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
    // Fallback OG card if tip not found
    return new ImageResponse(
      (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            width: "100%",
            height: "100%",
            backgroundColor: "#141416",
            color: "#FAFAFA",
            fontFamily: "monospace",
          }}
        >
          <div style={{ fontSize: 40, color: "#22D3EE" }}>TIPS.PAYGATE.FM</div>
          <div style={{ fontSize: 24, color: "#A1A1AA", marginTop: 16 }}>
            Tip not found
          </div>
        </div>
      ),
      { ...size }
    );
  }

  const amount = formatUsdc(tip.amount_usdc);
  const statusColor =
    tip.status === "paid"
      ? "#4ADE80"
      : tip.status === "escrowed"
        ? "#FBBF24"
        : tip.status === "claimed"
          ? "#22D3EE"
          : "#71717A";

  return new ImageResponse(
    (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          width: "100%",
          height: "100%",
          backgroundColor: "#141416",
          padding: 0,
          position: "relative",
        }}
      >
        {/* Top cyan gradient line */}
        <div
          style={{
            width: "100%",
            height: 3,
            background: "linear-gradient(90deg, #22D3EE, #06B6D4, #22D3EE)",
          }}
        />

        {/* Content */}
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            padding: "48px 64px",
            flex: 1,
            justifyContent: "space-between",
          }}
        >
          {/* Top section: amount + headline */}
          <div style={{ display: "flex", flexDirection: "column" }}>
            {/* Amount */}
            <div
              style={{
                fontSize: 96,
                fontWeight: 700,
                color: "#22D3EE",
                fontFamily: "monospace",
                lineHeight: 1,
                marginBottom: 8,
              }}
            >
              {amount}
            </div>
            <div
              style={{
                fontSize: 18,
                color: "#71717A",
                fontFamily: "monospace",
                letterSpacing: 2,
                textTransform: "uppercase" as const,
                marginBottom: 32,
              }}
            >
              USDC
            </div>

            {/* Headline */}
            <div
              style={{
                fontSize: 32,
                color: "#FAFAFA",
                fontWeight: 500,
                lineHeight: 1.3,
              }}
            >
              Agent tipped{" "}
              <span style={{ color: "#22D3EE" }}>@{tip.recipient_gh}</span> for{" "}
              {tip.package_name}
            </div>

            {/* Reason */}
            {tip.reason && (
              <div
                style={{
                  fontSize: 22,
                  color: "#A1A1AA",
                  fontStyle: "italic",
                  marginTop: 16,
                  borderLeft: "3px solid #22D3EE",
                  paddingLeft: 20,
                }}
              >
                &ldquo;{tip.reason}&rdquo;
              </div>
            )}
          </div>

          {/* Bottom row: status badge + branding */}
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "flex-end",
            }}
          >
            {/* Status badge */}
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
              }}
            >
              <div
                style={{
                  width: 10,
                  height: 10,
                  borderRadius: "50%",
                  backgroundColor: statusColor,
                }}
              />
              <span
                style={{
                  fontSize: 18,
                  color: statusColor,
                  fontFamily: "monospace",
                  textTransform: "uppercase" as const,
                  letterSpacing: 2,
                }}
              >
                {tip.status}
              </span>
            </div>

            {/* Branding */}
            <div
              style={{
                display: "flex",
                alignItems: "baseline",
                fontFamily: "monospace",
              }}
            >
              <span style={{ fontSize: 22, color: "#22D3EE", fontWeight: 700 }}>
                TIPS
              </span>
              <span style={{ fontSize: 22, color: "#71717A" }}>
                .PAYGATE.FM
              </span>
            </div>
          </div>
        </div>
      </div>
    ),
    { ...size }
  );
}
