import { Hono } from "hono";
import { db } from "#db";
import { board, list, card } from "#db/schema";
import { and, desc, eq, inArray } from "drizzle-orm";
import { env } from "cloudflare:workers";
import { ownerId } from "#api/lib/owner";
import { planCreate, targetIdOf } from "./projects.logic";
import { scopeProjects } from "./projects.scope";

/** The ids of the lists on the owner's boards. Cards have no owner column: a card is the owner's when its list is. */
const ownedListIds = (owner: string) =>
  db
    .select({ id: list.id })
    .from(list)
    .innerJoin(board, eq(list.boardId, board.id))
    .where(eq(board.userId, owner));

/** Whether the owner has this list, through its board. */
const ownsList = async (listId: string, owner: string): Promise<boolean> =>
  (
    await db
      .select({ id: list.id })
      .from(list)
      .innerJoin(board, eq(list.boardId, board.id))
      .where(and(eq(list.id, listId), eq(board.userId, owner)))
      .limit(1)
  ).length > 0;

/** Whether the owner has this board. */
const ownsBoard = async (boardId: string, owner: string): Promise<boolean> =>
  (
    await db
      .select({ id: board.id })
      .from(board)
      .where(and(eq(board.id, boardId), eq(board.userId, owner)))
      .limit(1)
  ).length > 0;

const app = new Hono()
  .get("/cards", async (c) => {
    const d1 = env?.DB;
    if (!d1) return c.json({ boards: [], lists: [], cards: [] });
    const userId = ownerId();
    try {
      const boards = await db.select().from(board);
      const lists = await db.select().from(list).orderBy(desc(list.order));
      const cards = await db.select().from(card).orderBy(desc(card.order));
      return c.json(scopeProjects(userId, { boards, lists, cards }));
    } catch (err: any) {
      return c.json({ error: err.message }, 500);
    }
  })
  .post("/cards", async (c) => {
    const d1 = env?.DB;
    if (!d1) return c.json({ error: "No D1 DB binding" }, 500);
    const userId = ownerId();
    const plan = planCreate(await c.req.json(), userId, crypto.randomUUID());
    if (!plan.ok) return c.json({ error: plan.error }, 400);
    // A list or card must sit inside one of the owner's boards.
    if (plan.type === "list" && !(await ownsBoard(plan.row.boardId, userId))) return c.json({ error: "Not found" }, 404);
    if (plan.type === "card" && !(await ownsList(plan.row.listId, userId))) return c.json({ error: "Not found" }, 404);
    try {
      if (plan.type === "board") await db.insert(board).values(plan.row);
      else if (plan.type === "list") await db.insert(list).values(plan.row);
      else await db.insert(card).values(plan.row);
      return c.json({ type: plan.type, ...plan.row }, 201);
    } catch (err: any) {
      return c.json({ error: err.message }, 500);
    }
  })
  .patch("/cards", async (c) => {
    const d1 = env?.DB;
    if (!d1) return c.json({ error: "No D1 DB binding" }, 500);
    const body = (await c.req.json()) as any;
    const owner = ownerId();
    const id = targetIdOf(body);
    if (!id) return c.json({ error: "Missing cardId" }, 400);
    // A card can only be moved into a list on one of the owner's boards.
    if (body.listId !== undefined && !(await ownsList(body.listId, owner))) {
      return c.json({ error: "Not found" }, 404);
    }
    try {
      await db
        .update(card)
        .set({
          ...(body.title !== undefined ? { title: body.title } : {}),
          ...(body.description !== undefined ? { description: body.description } : {}),
          ...(body.listId !== undefined ? { listId: body.listId } : {}),
          ...(body.order !== undefined ? { order: Number(body.order) } : {}),
          ...(body.customFields !== undefined ? { customFields: body.customFields } : {}),
          updatedAt: new Date(),
        })
        .where(and(eq(card.id, id), inArray(card.listId, ownedListIds(owner))));
      return c.json({ success: true });
    } catch (err: any) {
      return c.json({ error: err.message }, 500);
    }
  })
  .delete("/cards", async (c) => {
    const d1 = env?.DB;
    if (!d1) return c.json({ error: "No D1 DB binding" }, 500);
    const cardId = c.req.query("cardId");
    if (!cardId) return c.json({ error: "Missing cardId" }, 400);
    try {
      const owner = ownerId();
      await db.delete(card).where(and(eq(card.id, cardId), inArray(card.listId, ownedListIds(owner))));
      return c.json({ success: true });
    } catch (err: any) {
      return c.json({ error: err.message }, 500);
    }
  });

export default app;
