export interface LlmProvider {
  stream(systemPrompt: string, userMessage: string): AsyncIterable<string>;
}
