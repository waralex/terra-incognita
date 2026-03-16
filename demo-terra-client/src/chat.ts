import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import type { Config } from "./config.js";
import { TerraClient, type TransactionReq } from "./terra.js";
import type { LlmProvider } from "./llm/index.js";
import { buildContext } from "./context.js";
import { parseResponse } from "./parse.js";

export type ChatEvent =
  | { type: "delta"; text: string }
  | { type: "answer"; text: string }
  | { type: "transaction"; result: unknown }
  | { type: "error"; error: string }
  | { type: "done" };

const __dirname = dirname(fileURLToPath(import.meta.url));

let cachedPrompt: string | undefined;
function loadPrompt(): string {
  if (!cachedPrompt) {
    cachedPrompt = readFileSync(join(__dirname, "..", "prompts", "system.md"), "utf-8");
  }
  return cachedPrompt;
}

export async function handleChat(
  terra: TerraClient,
  llm: LlmProvider,
  config: Config,
  userMessage: string,
  emit: (event: ChatEvent) => void,
): Promise<void> {
  let context: string;
  try {
    context = await buildContext(terra, config);
  } catch (e) {
    context = `# Branch: ${config.branch}\n\n(Could not fetch context from Terra)`;
  }

  const systemPrompt = loadPrompt() + "\n\n" + context;

  if (config.logLlm) {
    console.log("\n--- LLM REQUEST ---");
    console.log("System prompt:\n%s", systemPrompt);
    console.log("\nUser message:\n%s", userMessage);
    console.log("--- END REQUEST ---\n");
  }

  let fullText = "";
  try {
    for await (const delta of llm.stream(systemPrompt, userMessage)) {
      fullText += delta;
      emit({ type: "delta", text: delta });
    }
  } catch (e: any) {
    emit({ type: "error", error: `LLM error: ${e.message}` });
    emit({ type: "done" });
    return;
  }

  if (config.logLlm) {
    console.log("\n--- LLM RESPONSE ---");
    console.log(fullText);
    console.log("--- END RESPONSE ---\n");
  }

  const parsed = parseResponse(fullText);
  emit({ type: "answer", text: parsed.answer });

  const txReq: TransactionReq = {
    meta: {
      question: userMessage,
      answer: parsed.answer,
      reasoning: parsed.reasoning,
    },
  };

  if (parsed.create?.length) txReq.create = parsed.create;
  if (parsed.update?.length) txReq.update = parsed.update;
  if (parsed.touch?.length) txReq.touch = parsed.touch;

  const hasMutations = parsed.create?.length || parsed.update?.length || parsed.touch?.length;

  if (hasMutations) {
    try {
      const result = await terra.transaction(config.branch, txReq);
      emit({ type: "transaction", result });
    } catch (e: any) {
      emit({ type: "error", error: `Transaction error: ${e.message}` });
    }
  }

  emit({ type: "done" });
}
