export interface Config {
  terraServerUrl: string;
  branch: string;
  port: number;
  llmProvider: string;
  llmModel: string;
  anthropicApiKey: string;
  openaiApiKey: string;
  contextTransactions: number;
  contextEntities: number;
}

export function loadConfig(): Config {
  return {
    terraServerUrl: env("TERRA_SERVER_URL", "http://localhost:3000"),
    branch: env("TERRA_BRANCH", "main"),
    port: parseInt(env("PORT", "3001"), 10),
    llmProvider: env("LLM_PROVIDER", "anthropic"),
    llmModel: env("LLM_MODEL", ""),
    anthropicApiKey: env("ANTHROPIC_API_KEY", ""),
    openaiApiKey: env("OPENAI_API_KEY", ""),
    contextTransactions: parseInt(env("CONTEXT_TRANSACTIONS", "10"), 10),
    contextEntities: parseInt(env("CONTEXT_ENTITIES", "20"), 10),
  };
}

function env(key: string, fallback: string): string {
  return process.env[key] ?? fallback;
}
