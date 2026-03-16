import type { Config } from "./config.js";
import type { TerraClient, EntityRes, TransactionRes } from "./terra.js";

export async function buildContext(terra: TerraClient, config: Config): Promise<string> {
  const [entities, transactions] = await Promise.all([
    terra.touchedEntities(config.branch, config.contextEntities).catch(() => []),
    terra.listTransactions(config.branch, config.contextTransactions).catch(() => []),
  ]);

  const parts: string[] = [];

  parts.push(`# Branch: ${config.branch}`);

  if (entities.length > 0) {
    parts.push("");
    parts.push("# Recent entities (by last touch)");
    for (const e of entities) {
      parts.push(formatEntity(e));
    }
  }

  if (transactions.length > 0) {
    parts.push("");
    parts.push("# Recent transactions");
    for (const tx of transactions) {
      parts.push(formatTransaction(tx));
    }
  }

  return parts.join("\n");
}

function formatEntity(e: EntityRes): string {
  const lines: string[] = [`## ${e.slug}`];
  if (e.description != null) {
    lines.push(`description: ${JSON.stringify(e.description)}`);
  }
  if (e.properties?.length > 0) {
    lines.push("properties:");
    for (const p of e.properties) {
      const reasoning = p.context.reasoning ? ` (reasoning: ${JSON.stringify(p.context.reasoning)})` : "";
      lines.push(`  ${p.property}: ${JSON.stringify(p.value)}${reasoning}`);
    }
  }
  return lines.join("\n");
}

function formatTransaction(tx: TransactionRes): string {
  const meta = tx.meta;
  const parts: string[] = [`- tx ${tx.context.tx_id.slice(0, 8)}`];
  if (meta.question) parts.push(`question=${JSON.stringify(meta.question)}`);
  if (meta.answer) parts.push(`answer=${JSON.stringify(meta.answer)}`);
  if (meta.reasoning) parts.push(`reasoning=${JSON.stringify(meta.reasoning)}`);
  return parts.join(" ");
}
