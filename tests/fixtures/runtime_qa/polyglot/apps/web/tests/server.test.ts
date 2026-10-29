import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import { after, before, test } from "node:test";
import { createRequestListener } from "../src/server.ts";

let server: Server;
let baseUrl: string;

before(async () => {
  server = createServer(createRequestListener());
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  baseUrl = `http://127.0.0.1:${address.port}`;
});

test("fixture server serves the form and saves a non-empty message", async () => {
  const page = await fetch(`${baseUrl}/`);
  assert.equal(page.status, 200);
  const pageText = await page.text();
  assert.match(pageText, /<label for="message">Message<\/label>/);
  assert.match(pageText, /data-testid="status"/);

  const saved = await fetch(`${baseUrl}/save`, { method: "POST", body: "offline QA input" });
  assert.equal(saved.status, 200);
  assert.equal(await saved.text(), "Saved");
});

test("fixture server rejects an empty save", async () => {
  const response = await fetch(`${baseUrl}/save`, { method: "POST", body: "  " });
  assert.equal(response.status, 400);
});

after(async () => {
  if (!server?.listening) return;
  await new Promise<void>((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
  });
});
