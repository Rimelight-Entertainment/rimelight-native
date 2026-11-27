export const CARD_TYPES = ["board", "list", "card"] as const;

export type CreatePlan =
  | {
      ok: true;
      type: "board";
      row: { id: string; userId: string; title: string; description: string | null };
    }
  | {
      ok: true;
      type: "list";
      row: { id: string; boardId: string; title: string; order: number };
    }
  | {
      ok: true;
      type: "card";
      row: {
        id: string;
        listId: string;
        title: string;
        description: string | null;
        order: number;
        customFields: Record<string, unknown>;
      };
    }
  | { ok: false; error: string };

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/**
 * Builds the row for `POST /api/projects/cards`. `type` selects the table: a board, a list
 * (needs `boardId`) or a card (needs `listId`). A missing `type` keeps the old behaviour of
 * creating a card.
 */
export function planCreate(body: unknown, userId: string, id: string): CreatePlan {
  const b = isRecord(body) ? body : {};
  const type = b.type ?? "card";
  if (typeof type !== "string" || !(CARD_TYPES as readonly string[]).includes(type)) {
    return { ok: false, error: "Invalid type" };
  }
  const title = typeof b.title === "string" ? b.title.trim() : "";
  if (!title) return { ok: false, error: "Missing title" };
  const description = typeof b.description === "string" ? b.description : null;
  const order = Number(b.order) || 0;

  if (type === "board") {
    return { ok: true, type: "board", row: { id, userId, title, description } };
  }
  if (type === "list") {
    if (typeof b.boardId !== "string" || !b.boardId) return { ok: false, error: "Missing boardId" };
    return { ok: true, type: "list", row: { id, boardId: b.boardId, title, order } };
  }
  if (typeof b.listId !== "string" || !b.listId) return { ok: false, error: "Missing listId" };
  return {
    ok: true,
    type: "card",
    row: {
      id,
      listId: b.listId,
      title,
      description,
      order,
      customFields: isRecord(b.customFields) ? b.customFields : {},
    },
  };
}

/** The card id a PATCH targets. The web sends `cardId`; the API originally read only `id`. */
export function targetIdOf(body: unknown): string | undefined {
  if (!isRecord(body)) return undefined;
  const raw = body.id ?? body.cardId;
  return typeof raw === "string" && raw ? raw : undefined;
}
