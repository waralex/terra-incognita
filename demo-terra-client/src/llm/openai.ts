import OpenAI from "openai";
import type { LlmProvider } from "./types.js";

export class OpenAIProvider implements LlmProvider {
  private client: OpenAI;
  private model: string;

  constructor(apiKey: string, model?: string) {
    this.client = new OpenAI({ apiKey });
    this.model = model || "gpt-4o";
  }

  async *stream(systemPrompt: string, userMessage: string): AsyncIterable<string> {
    const stream = await this.client.chat.completions.create({
      model: this.model,
      stream: true,
      messages: [
        { role: "system", content: systemPrompt },
        { role: "user", content: userMessage },
      ],
    });

    for await (const chunk of stream) {
      const delta = chunk.choices[0]?.delta?.content;
      if (delta) yield delta;
    }
  }
}
