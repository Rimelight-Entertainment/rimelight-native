import { Hono } from "hono";
import financesRoutes from "./routes/finances";
import groceriesRoutes from "./routes/groceries";
import habitsRoutes from "./routes/habits";
import healthRoutes from "./routes/health";
import housingRoutes from "./routes/housing";
import musicRoutes from "./routes/music";
import notesRoutes from "./routes/notes";
import notebooksRoutes from "./routes/notebooks";
import projectsRoutes from "./routes/projects";
import storageRoutes from "./routes/storage";
import todosRoutes from "./routes/todos";
import watchlistRoutes from "./routes/watchlist";
import { app as inboxApp } from "#workers/inbox";
import { requireAccess } from "#api/lib/auth";
import { meta } from "#api/meta";
import { env } from "cloudflare:workers";

// Every route is behind Cloudflare Access (checked again here, defence in depth). The paths are the same as the native
// client expects, under /api: /api/todos, /api/notes, and so on. The mail API keeps its own /api/v1/* paths, so the
// request reaches the inbox app unchanged.
const api = new Hono()
  .use("*", requireAccess)
  .route("/meta", meta)
  .route("/finances", financesRoutes)
  .route("/groceries", groceriesRoutes)
  .route("/habits", habitsRoutes)
  .route("/health", healthRoutes)
  .route("/housing", housingRoutes)
  .route("/music", musicRoutes)
  .route("/notes", notesRoutes)
  .route("/notebooks", notebooksRoutes)
  .route("/projects", projectsRoutes)
  .route("/storage", storageRoutes)
  .route("/todos", todosRoutes)
  .route("/watchlist", watchlistRoutes)
  .all("/v1/*", (c) => {
    const inboxEnv: any = { ...env, BUCKET: (env as any)?.PERSONAL_BLOB };
    return inboxApp.fetch(c.req.raw, inboxEnv, {
      waitUntil: (promise: Promise<any>) => promise.catch(() => {}),
      passThroughOnException: () => {},
    } as any);
  });

export type ApiType = typeof api;
export default api;
