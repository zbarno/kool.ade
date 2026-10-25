import { createServer, type IncomingMessage, type RequestListener, type ServerResponse } from "node:http";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const PAGE = await readFile(new URL("./index.html", import.meta.url), "utf8");

export function createRequestListener(): RequestListener {
  return (request, response): void => {
    if (request.method === "GET" && request.url === "/") {
      respond(response, 200, "text/html; charset=utf-8", PAGE);
      return;
    }
    if (request.method === "POST" && request.url === "/save") {
      void saveMessage(request, response);
      return;
    }
    respond(response, 404, "text/plain; charset=utf-8", "Not found");
  };
}

function respond(
  response: ServerResponse,
  status: number,
  contentType: string,
  body: string,
): void {
  response.writeHead(status, {
    "content-type": contentType,
    "content-length": Buffer.byteLength(body),
  });
  response.end(body);
}

async function saveMessage(request: IncomingMessage, response: ServerResponse): Promise<void> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(Buffer.from(chunk));
  const message = Buffer.concat(chunks).toString("utf8");
  if (message.trim() === "") {
    respond(response, 400, "text/plain; charset=utf-8", "Message required");
    return;
  }
  respond(response, 200, "text/plain; charset=utf-8", "Saved");
}

function start(): void {
  const host = process.env.HOST ?? "127.0.0.1";
  const port = Number(process.env.PORT ?? "4173");
  createServer(createRequestListener()).listen(port, host);
}

const invokedPath = process.argv[1];
if (invokedPath && import.meta.url === pathToFileURL(resolve(invokedPath)).href) start();
