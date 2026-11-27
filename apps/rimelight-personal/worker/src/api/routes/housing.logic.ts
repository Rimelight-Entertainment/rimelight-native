/**
 * The wishlist GET body. The native client decodes `wishlist`; the web reads `items`. Both keys
 * carry the same list, so neither client breaks.
 */
export function wishlistResponse<T>(list: T[]): { wishlist: T[]; items: T[] } {
  return { wishlist: list, items: list };
}
