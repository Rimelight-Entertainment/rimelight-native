import { describe, expect, it } from "vite-plus/test";
import { decideAccess } from "./auth.logic";

const config = { teamDomain: "https://team.example.cloudflareaccess.com", policyAud: "aud-1" };

describe("decideAccess", () => {
  it("allows a verified Access identity with an email", async () => {
    const d = await decideAccess({
      config,
      identify: async () => ({ email: "owner@example.com" }),
    });
    expect(d).toEqual({ ok: true, email: "owner@example.com" });
  });

  it("returns 401 when no valid Access token is present", async () => {
    const d = await decideAccess({ config, identify: async () => null });
    expect(d).toMatchObject({ ok: false, status: 401 });
  });

  it("returns 403 when the identity has no email", async () => {
    const d = await decideAccess({ config, identify: async () => ({ email: undefined }) });
    expect(d).toMatchObject({ ok: false, status: 403 });
  });

  it("fails closed with 500 and never calls identify when TEAM_DOMAIN is missing", async () => {
    let called = false;
    const d = await decideAccess({
      config: { ...config, teamDomain: undefined },
      identify: async () => {
        called = true;
        return { email: "x@example.com" };
      },
    });
    expect(d).toMatchObject({ ok: false, status: 500 });
    expect(called).toBe(false);
  });

  it("fails closed with 500 when POLICY_AUD is missing", async () => {
    const d = await decideAccess({
      config: { ...config, policyAud: "" },
      identify: async () => ({ email: "x@example.com" }),
    });
    expect(d).toMatchObject({ ok: false, status: 500 });
  });

  it("returns 500 when verification throws", async () => {
    const d = await decideAccess({
      config,
      identify: async () => {
        throw new Error("jwks unavailable");
      },
    });
    expect(d).toMatchObject({ ok: false, status: 500 });
  });
});
