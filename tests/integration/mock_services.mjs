// Deterministic local fixtures for the integrated stack, never a real model service.
import { createServer } from "node:http";
import { createHmac, timingSafeEqual } from "node:crypto";
const counts = {
  embeddings: 0,
  generations: 0,
  grades: 0,
  webhooks: 0,
  objects: 0,
};
createServer(async (req, res) => {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const raw = Buffer.concat(chunks);
  const reply = (value, status = 200) => {
    res.writeHead(status, { "content-type": "application/json" });
    res.end(JSON.stringify(value));
  };
  if (req.url === "/counts") return reply(counts);
  if (req.url === "/webhook") {
    const expected = Buffer.from(
      "sha256=" +
        createHmac("sha256", "local-smoke-webhook-secret-32-bytes")
          .update(raw)
          .digest("hex"),
    );
    const signature = Buffer.from(req.headers["x-refract-signature"] ?? "");
    if (
      expected.length !== signature.length ||
      !timingSafeEqual(expected, signature)
    )
      return reply({ error: "signature" }, 403);
    const envelope = JSON.parse(raw.toString());
    if (
      !envelope.version ||
      envelope.id !== req.headers["x-refract-delivery-id"]
    )
      return reply({ error: "envelope" }, 400);
    counts.webhooks++;
    return reply({ accepted: true });
  }
  if (req.url?.startsWith("/smoke-bucket/")) {
    if (
      !req.headers.authorization?.startsWith("AWS4-HMAC-SHA256 ") ||
      req.headers["x-amz-security-token"] !== "local-session-token"
    )
      return reply({ error: "signature" }, 403);
    counts.objects++;
    return reply({ accepted: true });
  }
  const body = raw.length ? JSON.parse(raw.toString()) : {};
  if (req.url === "/embeddings") {
    counts.embeddings++;
    // Four stable lexical features; this fixture tests plumbing, not semantic quality.
    const text = JSON.stringify(body.input).toLowerCase();
    const vector = [
      1,
      /refund|return/.test(text) ? 2 : 0,
      /shipping|delivery/.test(text) ? 2 : 0,
      /support/.test(text) ? 1 : 0,
    ];
    return reply({ data: [{ embedding: vector }] });
  }
  if (req.url === "/chat") {
    const grading = JSON.stringify(body.messages).includes(
      "Compare the two outputs using the rubric",
    );
    if (grading) counts.grades++;
    else counts.generations++;
    const content = grading
      ? JSON.stringify({
          score: 0.2,
          equivalent: false,
          reason: "The return window changed from 30 days to 14 days.",
        })
      : "You can return your order within 30 days of delivery.";
    return reply({
      model: "local-demo-model",
      choices: [{ message: { role: "assistant", content } }],
      usage: { prompt_tokens: 42, completion_tokens: 14, total_tokens: 56 },
    });
  }
  reply({ error: "unknown route" }, 404);
}).listen(8080, "0.0.0.0");
