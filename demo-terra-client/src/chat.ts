import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import type { Config } from "./config.js";
import { TerraClient, type TransactionReq } from "./terra.js";
import type { LlmProvider } from "./llm/index.js";
import { buildContext } from "./context.js";
import { parseResponse, type ParsedResponse } from "./parse.js";
// HACK: remove this import to disable translation
import { translateToEnglish } from "./translate.js";

export type ChatEvent =
  | { type: "delta"; text: string }
  | { type: "translated"; text: string }
  | { type: "answer"; text: string; mutations: Record<string, unknown[]> }
  | { type: "transaction"; result: unknown }
  | { type: "error"; error: string }
  | { type: "done" };

const __dirname = dirname(fileURLToPath(import.meta.url));
const MAX_RETRIES = 2;

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
  rawMessage: string,
  emit: (event: ChatEvent) => void,
): Promise<void> {
  // HACK: translate user message to English before processing
  let userMessage = rawMessage;
  if (config.anthropicApiKey) {
    userMessage = await translateToEnglish(config.anthropicApiKey, rawMessage);
    if (userMessage !== rawMessage) {
      emit({ type: "translated", text: userMessage });
    }
  }

  let context: string;
  try {
    context = await buildContext(terra, config, userMessage);
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
  const mutations: Record<string, unknown[]> = {};
  if (parsed.write?.length) mutations.write = parsed.write;
  if (parsed.touch?.length) mutations.touch = parsed.touch;
  emit({ type: "answer", text: parsed.answer, mutations });

  // Try to commit, retry with error feedback if it fails.
  let lastParsed = parsed;
  let lastJson = fullText;
  for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
    const txReq = buildTransactionReq(userMessage, lastParsed);
    try {
      const result = await terra.transaction(config.branch, txReq);
      emit({ type: "transaction", result });
      emit({ type: "done" });
      return;
    } catch (e: any) {
      if (attempt >= MAX_RETRIES) {
        emit({ type: "error", error: `Transaction error (after ${MAX_RETRIES} retries): ${e.message}` });
        break;
      }

      console.log("[retry] attempt %d failed: %s", attempt + 1, e.message);
      emit({ type: "error", error: `Transaction error, retrying: ${e.message}` });

      const retryMessage = [
        `The original user question was: ${userMessage}`,
        "",
        "Your previous JSON response was:",
        "```",
        lastJson,
        "```",
        "",
        `This caused a transaction error: ${e.message}`,
        "",
        "Please fix the JSON and send the complete corrected response. Same format — a single JSON object with answer, reasoning, and any mutations.",
      ].join("\n");

      let retryText = "";
      try {
        for await (const delta of llm.stream(systemPrompt, retryMessage)) {
          retryText += delta;
        }
      } catch (retryErr: any) {
        emit({ type: "error", error: `LLM retry error: ${retryErr.message}` });
        break;
      }

      if (config.logLlm) {
        console.log("\n--- LLM RETRY %d ---", attempt + 1);
        console.log(retryText);
        console.log("--- END RETRY ---\n");
      }

      lastParsed = parseResponse(retryText);
      lastJson = retryText;
      // Update the displayed answer with the retry.
      emit({ type: "answer", text: lastParsed.answer, mutations: {} });
    }
  }

  emit({ type: "done" });
}

function buildTransactionReq(userMessage: string, parsed: ParsedResponse): TransactionReq {
  const txReq: TransactionReq = {
    meta: {
      question: userMessage,
      answer: parsed.answer,
      reasoning: parsed.reasoning,
    },
  };

  if (parsed.write?.length) txReq.write = parsed.write;
  if (parsed.delete?.length) txReq.delete = parsed.delete;
  if (parsed.touch?.length) txReq.touch = parsed.touch;

  if (parsed.create_rule) {
    const r = parsed.create_rule;
    txReq.create_managed = [{
      type_name: "rule",
      slug: r.slug,
      state: r.state || "draft",
      fields: { content: r.content, ...(r.rationale && { rationale: r.rationale }) },
    }];
  }
  if (parsed.update_rule) {
    const r = parsed.update_rule;
    txReq.update_managed = [{
      type_name: "rule",
      slug: r.slug,
      ...(r.state && { state: r.state }),
      fields: {
        ...(r.content && { content: r.content }),
        ...(r.rationale && { rationale: r.rationale }),
      },
    }];
  }

  return txReq;
}
