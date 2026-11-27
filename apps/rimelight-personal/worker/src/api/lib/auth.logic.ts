export type AccessConfig = {
  teamDomain?: string | undefined;
  policyAud?: string | undefined;
};

export type AccessDecision =
  | { ok: true; email: string }
  | { ok: false; status: 401 | 403 | 500; error: string; message: string };

/**
 * Decides whether a request to /api/* carries a valid Cloudflare Access identity.
 * Fails closed: missing TEAM_DOMAIN or POLICY_AUD is a 500, a failed verification (or a thrown
 * JWKS error) is a 500 or 401, and a verified identity without an email is a 403.
 * `identify` returns the verified Access identity, or null when there is no valid token.
 */
export async function decideAccess(input: {
  config: AccessConfig;
  identify: () => Promise<{ email?: string | undefined } | null>;
}): Promise<AccessDecision> {
  const { teamDomain, policyAud } = input.config;
  if (!teamDomain || !policyAud) {
    return {
      ok: false,
      status: 500,
      error: "Internal Server Error",
      message: "Server access control is not configured.",
    };
  }
  let identity: { email?: string | undefined } | null;
  try {
    identity = await input.identify();
  } catch {
    return {
      ok: false,
      status: 500,
      error: "Internal Server Error",
      message: "Access verification is unavailable.",
    };
  }
  if (!identity) {
    return {
      ok: false,
      status: 401,
      error: "Unauthorized",
      message: "A valid Cloudflare Access token is required.",
    };
  }
  if (!identity.email) {
    return { ok: false, status: 403, error: "Forbidden", message: "Access identity has no email." };
  }
  return { ok: true, email: identity.email };
}
