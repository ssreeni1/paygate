import { NextRequest, NextResponse } from "next/server";
import { getSession } from "@/lib/session";

const GATEWAY_URL = process.env.PAYGATE_GATEWAY_URL ?? "http://localhost:8402";
const INTERNAL_SECRET = process.env.PAYGATE_INTERNAL_SECRET ?? "";

export async function POST(request: NextRequest) {
  // Require GitHub authentication
  const session = getSession();
  if (!session) {
    return NextResponse.json(
      { error: "Not authenticated. Sign in with GitHub first." },
      { status: 401 }
    );
  }

  try {
    const body = await request.json();
    const { wallet_address } = body;

    if (!wallet_address) {
      return NextResponse.json(
        { error: "wallet_address is required" },
        { status: 400 }
      );
    }

    // Use the authenticated username from the session, NOT user input
    const result = await fetch(`${GATEWAY_URL}/paygate/internal/claim`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${INTERNAL_SECRET}`,
      },
      body: JSON.stringify({
        github_username: session.user,
        wallet_address,
      }),
    });

    const data = await result.json();
    if (!result.ok) {
      return NextResponse.json(data, { status: result.status });
    }

    return NextResponse.json({
      ...data,
      authenticated_as: session.user,
      orgs: session.orgs,
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : "Internal error";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}
