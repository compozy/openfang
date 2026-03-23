#!/usr/bin/env node

const fs = require("node:fs");
const readline = require("node:readline");

const args = process.argv.slice(2);
const statePath = process.env.ARKY_CODEX_FIXTURE_STATE;

if (args.includes("--version")) {
  process.stdout.write("codex-fixture 0.1.0\n");
  process.exit(0);
}

if (args[0] !== "app-server" || args[1] !== "--listen" || args[2] !== "stdio://") {
  process.stderr.write(`unsupported invocation: ${args.join(" ")}\n`);
  process.exit(2);
}

const state = loadState();
let nextThreadId = state.nextThreadId;
const threads = new Map(Object.entries(state.threads));

function writeFrame(frame) {
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", ...frame })}\n`);
}

function ensureThread(threadId) {
  if (!threads.has(threadId)) {
    threads.set(threadId, { turnCount: 0 });
    persistState();
  }
  return threads.get(threadId);
}

function extractPrompt(params) {
  const input = Array.isArray(params.input) ? params.input : [];
  return input
    .filter((item) => item && typeof item.text === "string")
    .map((item) => item.text)
    .join("\n")
    .trim();
}

function notify(method, params) {
  writeFrame({ method, params });
}

function respond(id, result) {
  writeFrame({ id, result });
}

function handleInitialize(frame) {
  respond(frame.id, {
    protocolVersion: 1,
    serverInfo: {
      name: "codex-fixture",
      version: "0.1.0",
    },
    capabilities: {},
  });
}

function handleThreadStart(frame) {
  const threadId = `thread-${nextThreadId}`;
  nextThreadId += 1;
  ensureThread(threadId);
  persistState();
  respond(frame.id, {
    thread: {
      id: threadId,
    },
  });
}

function handleThreadResume(frame) {
  const params = frame.params || {};
  const threadId = typeof params.threadId === "string" ? params.threadId : "thread-unknown";
  ensureThread(threadId);
  respond(frame.id, {
    thread: {
      id: threadId,
    },
  });
}

function handleTurnStart(frame) {
  const params = frame.params || {};
  const threadId = typeof params.threadId === "string" ? params.threadId : "thread-missing";
  const thread = ensureThread(threadId);
  thread.turnCount += 1;
  persistState();

  const prompt = extractPrompt(params);
  if (prompt.includes("__CRASH_AFTER_TURN_START__")) {
    respond(frame.id, { accepted: true });
    process.stderr.write("fixture crash requested\n");
    setImmediate(() => process.exit(1));
    return;
  }

  const messageId = `message-${threadId}-${thread.turnCount}`;
  const text = `turn=${thread.turnCount};echo=${prompt}`;

  respond(frame.id, {
    turn: {
      id: `turn-${threadId}-${thread.turnCount}`,
    },
  });
  notify("turn/started", { threadId });
  notify("item/started", {
    threadId,
    item: {
      id: messageId,
      type: "agentMessage",
      text: "",
    },
  });
  notify("item/agentMessage/delta", {
    threadId,
    delta: text,
  });
  notify("item/completed", {
    threadId,
    item: {
      id: messageId,
      type: "agentMessage",
      text,
    },
  });
  notify("turn/completed", { threadId });
}

function handleModelList(frame) {
  const params = frame.params || {};
  if (params.cursor === "cursor-2") {
    respond(frame.id, {
      data: [
        {
          id: "o4-mini",
          name: "o4-mini",
        },
        {
          id: "gpt-4o",
          displayName: "GPT-4o",
        },
      ],
      nextCursor: null,
    });
    return;
  }

  respond(frame.id, {
    models: [
      {
        id: "gpt-5",
        name: "gpt-5",
      },
      {
        id: "o4-mini",
        name: "o4-mini",
      },
    ],
    nextCursor: "cursor-2",
  });
}

function handleFrame(frame) {
  if (!frame || typeof frame !== "object") {
    return;
  }

  switch (frame.method) {
    case "initialize":
      handleInitialize(frame);
      break;
    case "initialized":
      break;
    case "thread/start":
      handleThreadStart(frame);
      break;
    case "thread/resume":
      handleThreadResume(frame);
      break;
    case "turn/start":
      handleTurnStart(frame);
      break;
    case "model/list":
      handleModelList(frame);
      break;
    default:
      if (Object.prototype.hasOwnProperty.call(frame, "id")) {
        writeFrame({
          id: frame.id,
          error: {
            code: -32601,
            message: `unsupported method: ${frame.method}`,
          },
        });
      }
      break;
  }
}

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

rl.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) {
    return;
  }

  handleFrame(JSON.parse(trimmed));
});

rl.on("close", () => {
  process.exit(0);
});

function loadState() {
  if (!statePath) {
    return {
      nextThreadId: 1,
      threads: {},
    };
  }

  try {
    const raw = fs.readFileSync(statePath, "utf8");
    const parsed = JSON.parse(raw);
    return {
      nextThreadId:
        typeof parsed.nextThreadId === "number" ? parsed.nextThreadId : 1,
      threads:
        parsed.threads && typeof parsed.threads === "object" ? parsed.threads : {},
    };
  } catch (_error) {
    return {
      nextThreadId: 1,
      threads: {},
    };
  }
}

function persistState() {
  if (!statePath) {
    return;
  }

  fs.writeFileSync(
    statePath,
    JSON.stringify({
      nextThreadId,
      threads: Object.fromEntries(threads.entries()),
    }),
    "utf8",
  );
}
