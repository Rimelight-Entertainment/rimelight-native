export type GroceryItemLike = { isBought?: boolean; stockStatus?: string; [k: string]: any };
export type GroceryCategoryLike = { items: GroceryItemLike[]; [k: string]: any };

/**
 * The web's "Reset Out-of-Stock" button (`resetAllToNeedBuy`): every item goes back to needing a
 * buy (not bought, out of stock), the same state `toggleBought` gives an unbought item.
 */
export function resetAllToNeedBuy<C extends GroceryCategoryLike>(categories: C[]): C[] {
  return categories.map((c) => ({
    ...c,
    items: c.items.map((i) => ({ ...i, isBought: false, stockStatus: "out-of-stock" })),
  }));
}

/** The web's "Clear Checked" button (`clearBought`): removes every bought item from every category. */
export function clearBought<C extends GroceryCategoryLike>(categories: C[]): C[] {
  return categories.map((c) => ({
    ...c,
    items: c.items.filter((i) => i.isBought !== true),
  }));
}
