import express from "express";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { loadConfig } from "./config.js";
import { TerraClient } from "./terra.js";
import { createProvider } from "./llm/index.js";
import { handleChat } from "./chat.js";
import { buildContext } from "./context.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const config = loadConfig();
const terra = new TerraClient(config.terraServerUrl);
const llm = createProvider(config);

const app = express();
app.use(express.json());
app.use(express.static(join(__dirname, "..", "public")));

app.post("/api/chat", async (req, res) => {
  const { message, branch } = req.body;
  if (!message || typeof message !== "string") {
    res.status(400).json({ error: "message is required" });
    return;
  }

  const effectiveConfig = branch ? { ...config, branch } : config;

  res.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache",
    Connection: "keep-alive",
  });

  const send = (data: unknown) => {
    res.write(`data: ${JSON.stringify(data)}\n\n`);
  };

  let aborted = false;
  req.on("close", () => { aborted = true; });

  try {
    for await (const event of handleChat(terra, llm, effectiveConfig, message)) {
      if (aborted) break;
      send(event);
    }
  } catch (e: any) {
    if (!aborted) {
      console.error("Chat error:", e);
      send({ type: "error", error: e.message });
      send({ type: "done" });
    }
  }

  res.end();
});

app.get("/api/context", async (req, res) => {
  const branch = (req.query.branch as string) || config.branch;
  try {
    const ctx = await buildContext(terra, { ...config, branch });
    res.type("text/plain").send(ctx);
  } catch (e: any) {
    res.status(500).json({ error: e.message });
  }
});

app.listen(config.port, () => {
  console.log(`terra-client running on http://localhost:${config.port}`);
  console.log(`  Terra server: ${config.terraServerUrl}`);
  console.log(`  Branch: ${config.branch}`);
  console.log(`  LLM: ${config.llmProvider}`);
});
