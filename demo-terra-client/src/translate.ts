// HACK: translate user message to English via Haiku before sending to main LLM.
// To remove: delete this file and remove the translateToEnglish call in chat.ts.

const API_URL = "https://api.anthropic.com/v1/messages";
const MODEL = "claude-haiku-4-5-20251001";

export async function translateToEnglish(
  apiKey: string,
  text: string,
): Promise<string> {
  const res = await fetch(API_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-api-key": apiKey,
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify({
      model: MODEL,
      max_tokens: 1024,
      messages: [{
        role: "user",
        content: `Translate the following text to English. Output ONLY the translation, nothing else. If the text is already in English, output it unchanged.\n\n${text}`,
      }],
    }),
  });

  if (!res.ok) {
    console.error("[translate] failed, status=%d, using original", res.status);
    return text;
  }

  const body = await res.json() as any;
  const translated = body.content?.[0]?.text?.trim();
  if (!translated) return text;

  console.log("[translate] %s → %s", text.slice(0, 60), translated.slice(0, 60));
  return translated;
}
