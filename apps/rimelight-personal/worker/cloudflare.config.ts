import { bindings, defineConfig, exports } from "cf/config";

// The worker for one Rimelight Personal instance. Nothing here names a deployment: the app writes the account, the
// database id, the bucket name, the owner and the Access values at deploy time (docs/MIGRATION.md, section 2).
// Placeholders stay until then, and a placeholder fails closed (every request is refused).
export default defineConfig({
  worker: {
    name: "rimelight-personal",
    compatibilityDate: "2026-10-01",
    entrypoint: "./src/fetch.ts",
    observability: { enabled: true },
    exports: {
      MailboxDO: exports.durableObject({ storage: "sqlite" }),
    },
    env: {
      DB: bindings.d1({ id: "d07175c2-421a-4c1f-8033-2b1fffe46b44", dev: { remote: true } }),
      PERSONAL_BLOB: bindings.r2({ name: "rimelight-personal", dev: { remote: true } }),
      EMAIL: bindings.sendEmail(),
      MAILBOX: bindings.durableObject({ worker: "rimelight-personal", exportName: "MailboxDO" }),
      // Set by the app at deploy time: the Access team, the application's AUD, and the owner (the user_id on the rows).
      TEAM_DOMAIN: bindings.text("https://idantitydotme.cloudflareaccess.com"),
      POLICY_AUD: bindings.text("37aad5cbaf4bc7cd20d68690b3028101c8fd288b592d789f47b65cce0077c65b"),
      PERSONAL_OWNER_ID: bindings.secret(),
      DOMAINS: bindings.text(""),
      EMAIL_ADDRESSES: bindings.json([]),
    },
  },
});
