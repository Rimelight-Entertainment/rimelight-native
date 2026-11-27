import { Hono } from "hono";
import api from "#api/server";
import { receiveEmail } from "#workers/inbox";
import type { Env as InboxEnv } from "#workers/inbox/types";
import { runTrashCleanup } from "#tasks/cleanup";

export { MailboxDO } from "./workers/inbox/durableObject/index";

/** The inbox worker reads its mailbox bucket as `BUCKET`; here that is the personal R2 bucket. */
const inboxEnv = (env: Env): InboxEnv =>
  ({ ...env, BUCKET: (env as any).PERSONAL_BLOB }) as unknown as InboxEnv;

// Never scheduled: no cron trigger is configured. The handler is kept for when one is added.
export const scheduled = async (_controller: ScheduledController, env: Env, ctx: ExecutionContext) => {
  if (env.DB) {
    ctx.waitUntil(runTrashCleanup(env.DB));
  }
};

export const email = async (event: { raw: ReadableStream; rawSize: number }, env: Env) => {
  try {
    await receiveEmail(event, inboxEnv(env));
  } catch (e) {
    console.error("Failed to process incoming email:", (e as Error).message, (e as Error).stack);
    throw e;
  }
};

const app = new Hono<{ Bindings: Env }>();
app.route("/api", api);
app.notFound((c) => c.json({ error: "not_found" }, 404));

export default app;
