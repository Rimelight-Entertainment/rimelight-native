import { describe, expect, it } from "vite-plus/test";
import { clearBought, resetAllToNeedBuy } from "./groceries.logic";

const categories = [
  {
    id: "c1",
    name: "Produce",
    items: [
      { id: "i1", name: "Apple", isBought: true, stockStatus: "in-stock" },
      { id: "i2", name: "Pear", isBought: false, stockStatus: "low" },
    ],
  },
  { id: "c2", name: "Dairy", items: [{ id: "i3", name: "Milk", isBought: true, stockStatus: "in-stock" }] },
];

describe("resetAllToNeedBuy", () => {
  it("marks every item as not bought and out of stock, keeping everything else", () => {
    const out = resetAllToNeedBuy(categories);
    expect(out[0]?.items).toEqual([
      { id: "i1", name: "Apple", isBought: false, stockStatus: "out-of-stock" },
      { id: "i2", name: "Pear", isBought: false, stockStatus: "out-of-stock" },
    ]);
    expect(out[1]?.items[0]?.isBought).toBe(false);
    expect(out[0]?.id).toBe("c1");
  });
});

describe("clearBought", () => {
  it("removes bought items and keeps the rest", () => {
    const out = clearBought(categories);
    expect(out[0]?.items.map((i) => i.id)).toEqual(["i2"]);
    expect(out[1]?.items).toEqual([]);
    expect(out.map((c) => c.id)).toEqual(["c1", "c2"]);
  });
});
