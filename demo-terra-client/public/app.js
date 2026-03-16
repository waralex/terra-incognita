const messagesEl = document.getElementById("messages");
const form = document.getElementById("chat-form");
const input = document.getElementById("input");
const sendBtn = document.getElementById("send-btn");
const branchEl = document.getElementById("branch");

const branch = new URLSearchParams(location.search).get("branch") || "main";
branchEl.textContent = branch;

let busy = false;

form.addEventListener("submit", (e) => {
  e.preventDefault();
  const text = input.value.trim();
  if (!text || busy) return;
  sendMessage(text);
});

input.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    form.dispatchEvent(new Event("submit"));
  }
});

// Auto-resize textarea
input.addEventListener("input", () => {
  input.style.height = "auto";
  input.style.height = Math.min(input.scrollHeight, 120) + "px";
});

function addMessage(role, text) {
  const el = document.createElement("div");
  el.className = `message ${role}`;
  el.textContent = text;
  messagesEl.appendChild(el);
  messagesEl.scrollTop = messagesEl.scrollHeight;
  return el;
}

async function sendMessage(text) {
  busy = true;
  sendBtn.disabled = true;
  input.value = "";
  input.style.height = "auto";

  addMessage("user", text);
  const assistantEl = addMessage("assistant", "");

  try {
    const res = await fetch("/api/chat", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message: text, branch }),
    });

    if (!res.ok) {
      const err = await res.json();
      addMessage("error", err.error || "Request failed");
      return;
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let fullAnswer = "";

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";

      for (const line of lines) {
        if (!line.startsWith("data: ")) continue;
        let data;
        try { data = JSON.parse(line.slice(6)); } catch { continue; }

        switch (data.type) {
          case "delta":
            assistantEl.textContent += data.text;
            messagesEl.scrollTop = messagesEl.scrollHeight;
            break;
          case "answer":
            fullAnswer = data.text;
            assistantEl.textContent = fullAnswer;
            break;
          case "transaction":
            addMessage("system", `tx ${data.result?.context?.tx_id?.slice(0, 8) || "ok"}`);
            break;
          case "error":
            addMessage("error", data.error);
            break;
        }
      }
    }
  } catch (e) {
    addMessage("error", e.message);
  } finally {
    busy = false;
    sendBtn.disabled = false;
    input.focus();
  }
}
