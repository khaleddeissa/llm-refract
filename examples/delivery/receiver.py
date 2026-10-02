"""Local signed webhook receiver. Set REFRACT_WEBHOOK_SECRET and REFRACT_INBOX_PATH."""

import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from refract.delivery import WebhookInbox

inbox = WebhookInbox(os.environ["REFRACT_INBOX_PATH"], secret=os.environ["REFRACT_WEBHOOK_SECRET"])


class Receiver(BaseHTTPRequestHandler):
    def do_POST(self):
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 17 * 1024 * 1024:
                self.send_error(413)
                return
            result = inbox.accept(
                self.rfile.read(length),
                signature=self.headers.get("X-Refract-Signature", ""),
                delivery_id=self.headers.get("X-Refract-Delivery-Id", ""),
            )
            self.send_response(200)
            self.end_headers()
            self.wfile.write(result.encode())
        except ValueError:
            self.send_error(400, "Invalid signed delivery")
        except Exception:
            self.send_error(503, "Inbox commit failed; retry required")


if __name__ == "__main__":
    try:
        ThreadingHTTPServer(("127.0.0.1", 8091), Receiver).serve_forever()
    finally:
        inbox.close()
