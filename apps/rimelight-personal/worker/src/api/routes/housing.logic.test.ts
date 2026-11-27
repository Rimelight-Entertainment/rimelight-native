import { describe, expect, it } from "vite-plus/test";
import { wishlistResponse } from "./housing.logic";

describe("wishlistResponse", () => {
  it("returns the same list under both keys the clients read", () => {
    const out = wishlistResponse([{ id: "w1" }]);
    expect(out.wishlist).toEqual([{ id: "w1" }]);
    expect(out.items).toEqual([{ id: "w1" }]);
  });
});
