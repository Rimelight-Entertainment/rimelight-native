/**
 * Verifies a Cloudflare Access JWT on a request. The Access application gates the path at the edge; this checks the
 * token again in the Worker (defence in depth). Replaces `cfAccessIdentifier` from `@rimelight/cms`, which the installed
 * version does not export.
 *
 * The token comes from the `Cf-Access-Jwt-Assertion` header, or from the `CF_Authorization` cookie. It must be RS256,
 * signed by a key in the team's certificates, issued by the team, meant for this application (AUD), and not expired.
 */

type Jwk = JsonWebKey & { kid?: string };
type Claims = { iss?: string; aud?: string | string[]; exp?: number; email?: string; sub?: string };

const KEY_TTL_MS = 60 * 60 * 1000;
const keyCache = new Map<string, { keys: Jwk[]; fetchedAt: number }>();

const decodeSegment = (segment: string): Uint8Array => {
  const base64 = segment.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  return Uint8Array.from(atob(padded), (ch) => ch.charCodeAt(0));
};

const readJson = <T>(segment: string): T => JSON.parse(new TextDecoder().decode(decodeSegment(segment))) as T;

/** The token from the request, or null. */
export function accessTokenOf(request: Request): string | null {
  const header = request.headers.get("Cf-Access-Jwt-Assertion");
  if (header) return header;
  const cookie = request.headers.get("Cookie") ?? "";
  const match = cookie.match(/(?:^|;\s*)CF_Authorization=([^;]+)/);
  return match ? decodeURIComponent(match[1]!) : null;
}

async function signingKeys(teamDomain: string, force: boolean): Promise<Jwk[]> {
  const cached = keyCache.get(teamDomain);
  if (!force && cached && Date.now() - cached.fetchedAt < KEY_TTL_MS) return cached.keys;
  const response = await fetch(`${teamDomain}/cdn-cgi/access/certs`);
  if (!response.ok) throw new Error(`certs answered ${response.status}`);
  const body = (await response.json()) as { keys?: Jwk[] };
  const keys = body.keys ?? [];
  keyCache.set(teamDomain, { keys, fetchedAt: Date.now() });
  return keys;
}

/** The verified claims, or null when the token is missing or invalid. Throws only when the key set cannot be read. */
export async function verifyAccessJwt(
  request: Request,
  teamDomain: string,
  policyAud: string,
): Promise<Claims | null> {
  const token = accessTokenOf(request);
  if (!token) return null;
  const parts = token.split(".");
  if (parts.length !== 3) return null;
  const [headerPart, payloadPart, signaturePart] = parts as [string, string, string];

  let header: { alg?: string; kid?: string };
  let claims: Claims;
  try {
    header = readJson(headerPart);
    claims = readJson(payloadPart);
  } catch {
    return null;
  }
  if (header.alg !== "RS256") return null;

  // A key that is not in the cached set may have been rotated: fetch once more before refusing.
  let jwk = (await signingKeys(teamDomain, false)).find((k) => k.kid === header.kid);
  if (!jwk) jwk = (await signingKeys(teamDomain, true)).find((k) => k.kid === header.kid);
  if (!jwk) return null;

  const key = await crypto.subtle.importKey(
    "jwk",
    { kty: jwk.kty, n: jwk.n, e: jwk.e, alg: "RS256", ext: true },
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["verify"],
  );
  const valid = await crypto.subtle.verify(
    "RSASSA-PKCS1-v1_5",
    key,
    decodeSegment(signaturePart),
    new TextEncoder().encode(`${headerPart}.${payloadPart}`),
  );
  if (!valid) return null;

  const audiences = Array.isArray(claims.aud) ? claims.aud : [claims.aud];
  if (claims.iss !== teamDomain) return null;
  if (!audiences.includes(policyAud)) return null;
  if (typeof claims.exp !== "number" || claims.exp * 1000 <= Date.now()) return null;
  return claims;
}
