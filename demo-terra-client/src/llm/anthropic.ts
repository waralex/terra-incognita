import type { LlmProvider } from "./types.js";

const API_URL = "https://api.anthropic.com/v1/messages";

export class AnthropicProvider implements LlmProvider {
  private apiKey: string;
  private model: string;

  constructor(apiKey: string, model?: string) {
    this.apiKey = apiKey;
    this.model = model || "claude-sonnet-4-20250514";
  }

  async *stream(systemPrompt: string, userMessage: string): AsyncIterable<string> {
    console.log("[anthropic] sending request, model=%s", this.model);

    const res = await fetch(API_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-api-key": this.apiKey,
        "anthropic-version": "2023-06-01",
      },
      body: JSON.stringify({
        model: this.model,
        max_tokens: 4096,
        stream: true,
        system: systemPrompt,
        messages: [{ role: "user", content: userMessage }],
      }),
    });

    console.log("[anthropic] response status=%d", res.status);

    if (!res.ok) {
      const body = await res.text();
      console.error("[anthropic] error:", body);
      throw new Error(`Anthropic API ${res.status}: ${body}`);
    }

    if (!res.body) throw new Error("No response body");

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let chunks = 0;

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() || "";

        for (const line of lines) {
          if (!line.startsWith("data: ")) continue;
          const data = line.slice(6);
          if (data === "[DONE]") continue;

          let event;
          try { event = JSON.parse(data); } catch { continue; }

          if (
            event.type === "content_block_delta" &&
            event.delta?.type === "text_delta"
          ) {
            chunks++;
            yield event.delta.text;
          }
        }
      }
    } finally {
      reader.releaseLock();
    }

    console.log("[anthropic] stream done, %d text chunks", chunks);
  }
}
