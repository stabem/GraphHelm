// No-window launcher fixture (#1290): one loopback HTTP server, no dependencies.
import { createServer } from "node:http";

const port = Number(process.argv[2]);
if (!Number.isInteger(port) || port <= 0 || port > 65535) {
  console.error("usage: node server.mjs <port>");
  process.exit(2);
}
const server = createServer((request, response) => {
  const ok = request.url === "/health.json";
  response.writeHead(ok ? 200 : 404, { "content-type": "application/json" });
  response.end(ok ? '{"status":"ok"}' : '{"status":"missing"}');
});
server.listen(port, "127.0.0.1", () => console.log(`listening ${port}`));
