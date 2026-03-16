import Anthropic from "@anthropic-ai/sdk";
import type { LlmProvider } from "./types.js";

export class AnthropicProvider implements LlmProvider {
  private client: Anthropic;
  private model: string;

  constructor(apiKey: string, model?: string) {
    this.client = new Anthropic({ apiKey });
    this.model = model || "claude-sonnet-4-20250514";
  }

  async *stream(systemPrompt: string, userMessage: string): AsyncIterable<string> {
    const stream = this.client.messages.stream({
      model: this.model,
      max_tokens: 4096,
      system: systemPrompt,
      messages: [{ role: "user", content: userMessage }],
    });

    for await (const event of stream) {
      if (
        event.type === "content_block_delta" &&
        event.delta.type === "text_delta"
      ) {
        yield event.delta.text;
      }
    }
  }
}
