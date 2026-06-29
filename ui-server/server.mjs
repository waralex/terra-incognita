#!/usr/bin/env node
// Standalone viewer for terra-incognita: serves index.html and proxies /query
// to a running terra-server. Lets the UI iterate without rebuilding the Rust
// binary. No dependencies — Node 18+ built-ins only.
//
//   PORT=8080 TERRA_URL=http://127.0.0.1:7373 node server.mjs

import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const PORT = Number(process.env.PORT ?? 8080);
const TERRA_URL = (process.env.TERRA_URL ?? 'http://127.0.0.1:7373').replace(/\/$/, '');
const HERE = dirname(fileURLToPath(import.meta.url));

async function proxyQuery(req, res) {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  try {
    const upstream = await fetch(`${TERRA_URL}/query`, {
      method: 'POST',
      headers: { 'content-type': req.headers['content-type'] ?? 'application/json' },
      body: Buffer.concat(chunks),
    });
    const text = await upstream.text();
    res.writeHead(upstream.status, {
      'content-type': upstream.headers.get('content-type') ?? 'application/json',
    });
    res.end(text);
  } catch (e) {
    res.writeHead(502, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: `proxy to ${TERRA_URL} failed: ${e.message}`, kind: 'proxy_error' }));
  }
}

const server = createServer(async (req, res) => {
  if (req.method === 'POST' && req.url === '/query') return proxyQuery(req, res);
  if (req.method === 'GET' && (req.url === '/' || req.url === '/index.html')) {
    try {
      const html = await readFile(join(HERE, 'index.html'));
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      res.end(html);
    } catch (e) {
      res.writeHead(500).end(String(e));
    }
    return;
  }
  res.writeHead(404).end('not found');
});

server.listen(PORT, () => {
  console.log(`terra ui → http://localhost:${PORT}  (proxying /query → ${TERRA_URL})`);
});
