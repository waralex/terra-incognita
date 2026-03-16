import type { Config } from "./config.js";
import type { TerraClient, EntityRes, TransactionRes } from "./terra.js";

export async function buildContext(terra: TerraClient, config: Config): Promise<string> {
  const [entities, transactions] = await Promise.all([
    terra.touchedEntities(config.branch, config.contextEntities).catch(() => []),
    terra.listTransactions(config.branch, config.contextTransactions).catch(() => []),
  ]);

  const parts: string[] = [];

  parts.push(`# Branch: ${config.branch}`);
  parts.push(`# Current time: ${formatTime(new Date().toISOString())}`);

  if (entities.length > 0) {
    parts.push("");
    parts.push("# Recent entities (by last touch)");
    for (const e of entities) {
      parts.push(formatEntity(e));
    }
  }

  if (transactions.length > 0) {
    parts.push("");
    parts.push("# Transaction history (oldest first)");
    for (const tx of transactions.reverse()) {
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

function formatTime(iso?: string): string {
  if (!iso) return "unknown";
  const d = new Date(iso);
  return d.toLocaleString("en-GB", {
    year: "numeric", month: "short", day: "numeric",
    hour: "2-digit", minute: "2-digit", second: "2-digit",
    hour12: false,
    timeZone: "UTC",
    timeZoneName: "short",
  });
}

function formatTransaction(tx: TransactionRes): string {
  const meta = tx.meta;
  const time = formatTime(tx.context.time);
  const parts: string[] = [`- [${time}]`];
  if (meta.question) parts.push(`question=${JSON.stringify(meta.question)}`);
  if (meta.answer) parts.push(`answer=${JSON.stringify(meta.answer)}`);
  if (meta.reasoning) parts.push(`reasoning=${JSON.stringify(meta.reasoning)}`);
  return parts.join(" ");
}
