import { NextRequest, NextResponse } from "next/server";
import { claimTips } from "@/lib/api";

export async function POST(request: NextRequest) {
  try {
    const body = await request.json();
    const { github_username, wallet_address } = body;

    if (!github_username || !wallet_address) {
      return NextResponse.json(
        { error: "github_username and wallet_address are required" },
        { status: 400 }
      );
    }

    const result = await claimTips(github_username, wallet_address);
    return NextResponse.json(result);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Internal error";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}
