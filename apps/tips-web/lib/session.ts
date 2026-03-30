import { cookies } from "next/headers";
import { createHmac } from "crypto";

const COOKIE_SECRET = process.env.COOKIE_SECRET ?? "dev-secret-change-me";

export interface GhSession {
  user: string;
  orgs: string[];
}

/** Read and verify the GitHub session from the signed cookie. */
export function getSession(): GhSession | null {
  const cookie = cookies().get("gh_session")?.value;
  if (!cookie) return null;

  const dotIdx = cookie.lastIndexOf(".");
  if (dotIdx === -1) return null;

  const payload = cookie.slice(0, dotIdx);
  const hmac = cookie.slice(dotIdx + 1);

  const expectedHmac = createHmac("sha256", COOKIE_SECRET)
    .update(Buffer.from(payload, "base64").toString())
    .digest("hex");

  if (hmac !== expectedHmac) return null;

  try {
    const data = JSON.parse(Buffer.from(payload, "base64").toString());
    if (!data.user) return null;
    return { user: data.user, orgs: data.orgs || [] };
  } catch {
    return null;
  }
}
