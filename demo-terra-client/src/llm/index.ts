import type { Config } from "../config.js";
import type { LlmProvider } from "./types.js";
import { AnthropicProvider } from "./anthropic.js";
import { OpenAIProvider } from "./openai.js";

export type { LlmProvider } from "./types.js";

export function createProvider(config: Config): LlmProvider {
  const model = config.llmModel || undefined;

  switch (config.llmProvider) {
    case "anthropic":
      if (!config.anthropicApiKey) throw new Error("ANTHROPIC_API_KEY is required");
      return new AnthropicProvider(config.anthropicApiKey, model);

    case "openai":
      if (!config.openaiApiKey) throw new Error("OPENAI_API_KEY is required");
      return new OpenAIProvider(config.openaiApiKey, model);

    default:
      throw new Error(`Unknown LLM provider: ${config.llmProvider}`);
  }
}
