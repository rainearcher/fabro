import { describe, test, expect, beforeEach, mock } from "bun:test";
import { apiJson } from "./api-client";

const originalFetch = globalThis.fetch;

beforeEach(() => {
  globalThis.fetch = originalFetch;
});

describe("apiJson", () => {
  test("returns parsed JSON on 200", async () => {
    globalThis.fetch = mock(() =>
      Promise.resolve(new Response(JSON.stringify({ id: 1, name: "test" }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }))
    );

    const result = await apiJson<{ id: number; name: string }>("/items/1");

    expect(result).toEqual({ id: 1, name: "test" });
  });

  test("throws Response with status 404 and null body on not found", async () => {
    globalThis.fetch = mock(() =>
      Promise.resolve(new Response("Not Found: /items/999", { status: 404 }))
    );

    try {
      await apiJson("/items/999");
      expect.unreachable("should have thrown");
    } catch (thrown) {
      expect(thrown).toBeInstanceOf(Response);
      const res = thrown as Response;
      expect(res.status).toBe(404);
      expect(res.body).toBeNull();
    }
  });

  test("throws Response with status 500 and null body, stripping sensitive details", async () => {
    globalThis.fetch = mock(() =>
      Promise.resolve(
        new Response(
          "Internal error: database connection string is postgres://admin:secret@db.internal:5432/prod",
          { status: 500 }
        )
      )
    );

    try {
      await apiJson("/items/1");
      expect.unreachable("should have thrown");
    } catch (thrown) {
      expect(thrown).toBeInstanceOf(Response);
      const res = thrown as Response;
      expect(res.status).toBe(500);
      expect(res.body).toBeNull();
    }
  });
});
