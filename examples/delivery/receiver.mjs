import { createServer } from "node:http";
import { openWebhookInbox } from "@llm-refract/sdk";
const inbox = await openWebhookInbox(
  process.env.REFRACT_INBOX_PATH,
  process.env.REFRACT_WEBHOOK_SECRET,
);
createServer(async (request, response) => {
  if (request.method !== "POST") {
    response.writeHead(405).end();
    return;
  }
  try {
    const chunks = [];
    let size = 0;
    for await (const chunk of request) {
      size += chunk.length;
      if (size > 17 * 1024 * 1024) {
        response.writeHead(413).end();
        return;
      }
      chunks.push(chunk);
    }
    const result = inbox.accept(
      Buffer.concat(chunks),
      String(request.headers["x-refract-signature"] ?? ""),
      String(request.headers["x-refract-delivery-id"] ?? ""),
    );
    response.writeHead(200).end(result);
  } catch {
    response.writeHead(400).end("Delivery was not acknowledged");
  }
}).listen(8091, "127.0.0.1");
