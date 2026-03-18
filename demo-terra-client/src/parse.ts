export interface ParsedResponse {
  answer: string;
  reasoning: string;
  create?: CreateItem[];
  update?: UpdateItem[];
  touch?: TouchItem[];
  create_rule?: RuleItem;
  update_rule?: RuleUpdate;
}

export interface CreateItem {
  slug: string;
  description?: unknown;
  properties?: { property: string; value: unknown }[];
  meta?: Record<string, unknown>;
}

export interface UpdateItem {
  slug: string;
  properties?: { property: string; value: unknown }[];
  meta?: Record<string, unknown>;
}

export interface TouchItem {
  entity: string;
  reasoning: string;
}

export interface RuleItem {
  slug: string;
  content: string;
  rationale?: string;
}

export interface RuleUpdate {
  slug: string;
  state?: string;
  content?: string;
  rationale?: string;
}

export function parseResponse(text: string): ParsedResponse {
  let json: any;

  // Try direct parse
  try {
    json = JSON.parse(text);
  } catch {
    const fallback: ParsedResponse = { answer: text, reasoning: "Failed to parse structured response" };

    // Try extracting from ```json ... ``` fence
    const match = text.match(/```json\s*\n([\s\S]*?)\n\s*```/);
    if (match) {
      try { json = JSON.parse(match[1]); } catch { return fallback; }
    } else {
      // Try finding first { ... } block
      const start = text.indexOf("{");
      const end = text.lastIndexOf("}");
      if (start !== -1 && end > start) {
        try { json = JSON.parse(text.slice(start, end + 1)); } catch { return fallback; }
      } else {
        return fallback;
      }
    }
  }

  return {
    answer: json.answer ?? text,
    reasoning: json.reasoning ?? "",
    create: json.create,
    update: json.update,
    touch: json.touch,
    create_rule: json.create_rule,
    update_rule: json.update_rule,
  };
}
