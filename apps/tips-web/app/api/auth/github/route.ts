import { NextRequest, NextResponse } from "next/server";
import { randomBytes, createHmac } from "crypto";

// GitHub OAuth App credentials (set in Vercel env vars)
const CLIENT_ID = process.env.GITHUB_CLIENT_ID ?? "";
const COOKIE_SECRET = process.env.COOKIE_SECRET ?? "dev-secret-change-me";

export async function GET(request: NextRequest) {
  if (!CLIENT_ID) {
    return NextResponse.json(
      { error: "GitHub OAuth not configured (GITHUB_CLIENT_ID missing)" },
      { status: 500 }
    );
  }

  // Generate CSRF state token
  const state = randomBytes(16).toString("hex");
  const stateHmac = createHmac("sha256", COOKIE_SECRET)
    .update(state)
    .digest("hex");

  const redirectUri = new URL("/api/auth/callback", request.url).toString();

  const githubUrl = new URL("https://github.com/login/oauth/authorize");
  githubUrl.searchParams.set("client_id", CLIENT_ID);
  githubUrl.searchParams.set("redirect_uri", redirectUri);
  githubUrl.searchParams.set("scope", "read:user read:org");
  githubUrl.searchParams.set("state", `${state}.${stateHmac}`);

  const response = NextResponse.redirect(githubUrl.toString());

  // Store state in cookie for CSRF verification
  response.cookies.set("gh_oauth_state", state, {
    httpOnly: true,
    secure: process.env.NODE_ENV === "production",
    sameSite: "lax",
    maxAge: 600, // 10 minutes
    path: "/",
  });

  return response;
}
