import type { Config } from "./config.js";
import type { TerraClient, EntityRes, TransactionRes, ManagedRes, SimilarEntityRes } from "./terra.js";

export async function buildContext(
  terra: TerraClient,
  config: Config,
  userMessage?: string,
): Promise<string> {
  const [allEntities, transactions, managed, similar] = await Promise.all([
    terra.touchedEntities(config.branch, config.contextEntities).catch(() => []),
    terra.listTransactions(config.branch, config.contextTransactions).catch(() => []),
    terra.listManaged(config.branch).catch(() => []),
    userMessage && config.similarEntities > 0
      ? terra.similarEntities(
          config.branch, [userMessage], config.similarEntities, config.similarMinScore,
        ).catch(() => [])
      : Promise.resolve([]),
  ]);

  if (similar.length > 0) {
    console.log("[similar] raw results: %s", similar.map((s) => `${s.slug}(${s.similarity.toFixed(3)}, q${s.matched_query})`).join(", "));
  }

  // Filter similar: exclude entities already in touched.
  const recentSlugs = new Set(allEntities.map((e) => e.slug));
  const relatedEntities: SimilarEntityRes[] = similar.filter((s) => !recentSlugs.has(s.slug));

  const rules = managed.filter((m) => m.type_name === "rule");

  const parts: string[] = [];

  parts.push(`# Branch: ${config.branch}`);
  parts.push(`# Current time: ${formatTime(new Date().toISOString())}`);

  if (rules.length > 0) {
    parts.push("");
    parts.push("# Active rules");
    for (const r of rules) {
      parts.push(formatRule(r));
    }
  }

  if (allEntities.length > 0) {
    parts.push("");
    parts.push("# Recent entities (by last touch)");
    for (const e of allEntities) {
      parts.push(formatEntity(e));
    }
  }

  if (relatedEntities.length > 0) {
    parts.push("");
    parts.push("# Possibly related to the question");
    for (const e of relatedEntities) {
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

function formatRule(r: ManagedRes): string {
  const state = r.state ? ` [${r.state}]` : "";
  const rationale = r.fields.rationale ? ` (rationale: ${JSON.stringify(r.fields.rationale)})` : "";
  return `- **${r.slug}**${state}: ${r.fields.content ?? ""}${rationale}`;
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
