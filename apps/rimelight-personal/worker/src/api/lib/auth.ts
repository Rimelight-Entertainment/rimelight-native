import { createMiddleware } from "hono/factory";
import { env } from "cloudflare:workers";
import { decideAccess } from "./auth.logic";
import { verifyAccessJwt } from "./access-jwt";

/**
 * Worker-side check for /api/*. The edge Access application already gates the path; this verifies the Access JWT again
 * (defence in depth). Fails closed: see `decideAccess`.
 */
export const requireAccess = createMiddleware(async (c, next) => {
  const e = env as any;
  const teamDomain: string | undefined = e?.TEAM_DOMAIN;
  const policyAud: string | undefined = e?.POLICY_AUD;
  const decision = await decideAccess({
    config: { teamDomain, policyAud },
    identify: async () => {
      if (!teamDomain || !policyAud) return null;
      const claims = await verifyAccessJwt(c.req.raw, teamDomain, policyAud);
      return claims ? { email: claims.email } : null;
    },
  });
  if (!decision.ok) {
    return c.json({ error: decision.error, message: decision.message }, decision.status);
  }
  return next();
});
