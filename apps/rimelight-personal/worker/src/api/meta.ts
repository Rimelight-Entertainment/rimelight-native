import { Hono } from "hono";
import { env } from "cloudflare:workers";

/** The backend release. The app compares it with the version it was built against (docs/MIGRATION.md, R3.4). */
export const BACKEND_VERSION = "0.1.0";
/** The migration set this worker expects, named after its file in `drizzle/`. */
export const SCHEMA_VERSION = "0000_personal_backend";

/**
 * `GET /api/meta`: what this instance is and which optional modules are configured. The app reads it on start to know
 * whether the backend needs an update, and to hide modules that are not set up (R2.4, R4.3).
 */
export const meta = new Hono().get("/", (c) => {
  const e = env as any;
  const domains = String(e?.DOMAINS ?? "")
    .split(",")
    .map((d: string) => d.trim())
    .filter(Boolean);
  return c.json({
    backend: BACKEND_VERSION,
    schema: SCHEMA_VERSION,
    modules: {
      files: Boolean(e?.PERSONAL_BLOB),
      mail: domains.length > 0 && Boolean(e?.EMAIL),
    },
  });
});
