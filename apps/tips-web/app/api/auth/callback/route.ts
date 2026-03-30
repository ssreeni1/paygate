import { NextRequest, NextResponse } from "next/server";
import { createHmac } from "crypto";

const CLIENT_ID = process.env.GITHUB_CLIENT_ID ?? "";
const CLIENT_SECRET = process.env.GITHUB_CLIENT_SECRET ?? "";
const COOKIE_SECRET = process.env.COOKIE_SECRET ?? "dev-secret-change-me";

export async function GET(request: NextRequest) {
  const { searchParams } = new URL(request.url);
  const code = searchParams.get("code");
  const state = searchParams.get("state");

  if (!code || !state) {
    return NextResponse.redirect(new URL("/claim?error=missing_params", request.url));
  }

  // Verify CSRF state
  const savedState = request.cookies.get("gh_oauth_state")?.value;
  const [stateValue, stateHmac] = state.split(".");
  const expectedHmac = createHmac("sha256", COOKIE_SECRET)
    .update(stateValue)
    .digest("hex");

  if (!savedState || savedState !== stateValue || stateHmac !== expectedHmac) {
    return NextResponse.redirect(new URL("/claim?error=invalid_state", request.url));
  }

  // Exchange code for access token
  const tokenResp = await fetch("https://github.com/login/oauth/access_token", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
    body: JSON.stringify({
      client_id: CLIENT_ID,
      client_secret: CLIENT_SECRET,
      code,
    }),
  });

  const tokenData = await tokenResp.json();
  if (!tokenData.access_token) {
    return NextResponse.redirect(new URL("/claim?error=token_failed", request.url));
  }

  // Get the authenticated user's username
  const userResp = await fetch("https://api.github.com/user", {
    headers: {
      Authorization: `Bearer ${tokenData.access_token}`,
      Accept: "application/vnd.github+json",
      "User-Agent": "agent-tips/0.6",
    },
  });

  const userData = await userResp.json();
  if (!userData.login) {
    return NextResponse.redirect(new URL("/claim?error=user_failed", request.url));
  }

  const username = userData.login.toLowerCase();

  // Also fetch the user's orgs (includes private ones with read:org scope)
  const orgsResp = await fetch("https://api.github.com/user/orgs", {
    headers: {
      Authorization: `Bearer ${tokenData.access_token}`,
      Accept: "application/vnd.github+json",
      "User-Agent": "agent-tips/0.6",
    },
  });

  let orgLogins: string[] = [];
  try {
    const orgsData = await orgsResp.json();
    if (Array.isArray(orgsData)) {
      orgLogins = orgsData.map((o: { login: string }) => o.login.toLowerCase());
    }
  } catch {}

  // Create a signed session cookie with username + orgs
  const sessionData = JSON.stringify({ user: username, orgs: orgLogins });
  const sessionHmac = createHmac("sha256", COOKIE_SECRET)
    .update(sessionData)
    .digest("hex");
  const sessionCookie = `${Buffer.from(sessionData).toString("base64")}.${sessionHmac}`;

  const response = NextResponse.redirect(new URL("/claim", request.url));

  response.cookies.set("gh_session", sessionCookie, {
    httpOnly: true,
    secure: process.env.NODE_ENV === "production",
    sameSite: "lax",
    maxAge: 3600, // 1 hour
    path: "/",
  });

  // Clear the OAuth state cookie
  response.cookies.delete("gh_oauth_state");

  return response;
}
