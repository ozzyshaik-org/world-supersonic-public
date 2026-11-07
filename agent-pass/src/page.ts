import type { Operation } from "./types.js";

export function escapeHtml(value: unknown): string {
  return String(value).replace(/[&<>"']/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[character]!);
}

function hidden(name: string, value: string): string {
  return `<input type="hidden" name="${escapeHtml(name)}" value="${escapeHtml(value)}">`;
}

export function page(title: string, content: string): string {
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${escapeHtml(title)} · Supersonic</title>
<style>
:root{color-scheme:light;--paper:oklch(99% .003 255);--ink:oklch(24% .025 255);--muted:oklch(43% .025 255);--line:oklch(88% .015 255);--blue:oklch(52% .21 263);--blue-dark:oklch(42% .19 263);--soft:oklch(96% .018 255)}
*{box-sizing:border-box}body{margin:0;background:var(--paper);color:var(--ink);font:16px/1.65 "Fira Sans","IBM Plex Sans",sans-serif}main{width:min(100% - 32px,760px);margin:0 auto;padding:48px 0 64px}header{border-bottom:1px solid var(--line);padding-bottom:24px;margin-bottom:32px}h1{font-size:clamp(28px,6vw,42px);line-height:1.12;letter-spacing:-.035em;margin:8px 0 16px}h2{font-size:20px;margin:32px 0 8px}p{max-width:70ch}.brand{color:var(--blue);font-size:13px;font-weight:600;letter-spacing:.13em;text-transform:uppercase}.status{display:inline-block;background:var(--soft);color:var(--blue-dark);padding:3px 9px;border-radius:3px;font-size:13px;font-weight:600}.notice{border-left:2px solid var(--blue);padding:8px 16px;background:var(--soft);margin:24px 0}dl{display:grid;grid-template-columns:minmax(110px,180px) 1fr;gap:8px 16px;margin:24px 0}dt{color:var(--muted)}dd{margin:0;min-width:0;overflow-wrap:anywhere}code,pre{font-family:"Cascadia Code",ui-monospace,monospace}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:var(--soft);padding:16px;max-height:320px;overflow:auto}form{margin:12px 0}.actions{display:flex;flex-wrap:wrap;gap:12px;align-items:center}.actions form{margin:0}button,input{font:inherit}button{min-height:44px;border:1px solid var(--blue);border-radius:3px;background:var(--blue);color:var(--paper);padding:9px 16px;font-weight:600;cursor:pointer;transition:background-color 160ms ease-out,transform 160ms ease-out}button:hover{background:var(--blue-dark)}button:active{transform:scale(.98)}button.secondary{background:transparent;color:var(--blue-dark);border-color:var(--line)}button.secondary:hover{background:var(--soft)}button:focus-visible,a:focus-visible,input:focus-visible{outline:3px solid var(--blue);outline-offset:3px}a{color:var(--blue-dark)}input[type=text]{min-height:44px;width:100%;padding:8px 10px;border:1px solid var(--line);border-radius:3px;background:var(--paper);color:var(--ink)}label{display:block;color:var(--muted);margin:16px 0 4px}.fine{font-size:13px;color:var(--muted)}footer{border-top:1px solid var(--line);margin-top:48px;padding-top:16px}@media(max-width:480px){main{padding-top:28px}dl{grid-template-columns:1fr;gap:0 0}dd{margin-bottom:12px}.actions{align-items:stretch;flex-direction:column}.actions button{width:100%}}@media(prefers-reduced-motion:reduce){button{transition:none}}
</style></head><body><main>${content}<footer class="fine">Experimental Agent Pass checkpoint · Elevated read only · No Arc payment is approved or submitted here.</footer></main></body></html>`;
}

export function demoPage(accountId: string): string {
  return page("Local demo session", `<header><div class="brand">Supersonic · Local demo</div><h1>Open a synthetic account session</h1></header>
<p>This creates a local session for <strong>${escapeHtml(accountId)}</strong>. It is not World ID authentication and not production account evidence.</p>
<form method="post" action="/demo/session"><button type="submit">Create local demo session</button></form>`);
}

export function accountPage(account: { id: string; worldLinked: boolean }, csrf: string): string {
  const link = account.worldLinked
    ? `<p class="status">World ID linked</p><p>The linked issuer/subject pair is stored server-side and is not displayed or published.</p>`
    : `<p>No World ID is linked. Link one from this authenticated application session before starting a checkpoint.</p>
       <form method="post" action="/account/link/start">${hidden("csrf", csrf)}<button type="submit">Link with World ID</button></form>`;
  return page("Agent Pass account", `<header><div class="brand">Supersonic · Agent Pass</div><h1>Human checkpoint account</h1><p><strong>${escapeHtml(account.id)}</strong></p></header>
${link}<h2>Open an agent request</h2><form method="get" action="/operations/find"><label for="operation">Operation ID</label><input id="operation" name="id" type="text" required><button type="submit">Open stored request</button></form>`);
}

export function operationPage(operation: Operation, csrf: string): string {
  const common = `${hidden("csrf", csrf)}${hidden("fingerprint", operation.fingerprint)}`;
  let controls = "";
  let notice = "";
  if (["proposed", "awaiting_auth"].includes(operation.state)) {
    controls = `<form method="post" action="/operations/${operation.id}/auth/start">${common}<button type="submit">Authenticate fresh with World ID</button></form>${cancelForm(operation, common)}`;
    notice = "The protected read is paused. Authentication does not itself approve this request.";
  } else if (operation.state === "awaiting_approval") {
    controls = `<div class="actions"><form method="post" action="/operations/${operation.id}/decision">${common}${hidden("decision", "approve")}<button type="submit">Approve this exact request</button></form><form method="post" action="/operations/${operation.id}/decision">${common}${hidden("decision", "deny")}<button class="secondary" type="submit">Deny</button></form></div>${cancelForm(operation, common)}`;
    notice = "World ID authentication was validated for this attempt. Review the stored request; approval is separate and single-use.";
  } else if (operation.state === "approved") {
    controls = `<form method="post" action="/operations/${operation.id}/resume">${common}<button type="submit">Run approved read once</button></form>${cancelForm(operation, common)}`;
    notice = "Approval is recorded. Execution rechecks identity, freshness, expiry and the exact fingerprint.";
  } else if (operation.state === "executed") {
    notice = "The approved elevated read executed once. Repeating resume returns this stored response without another upstream read.";
  } else {
    notice = `No protected action is available in state ${operation.state}.`;
  }
  const response = operation.response === undefined ? "" : `<h2>Stored protected response</h2><pre>${escapeHtml(JSON.stringify(operation.response, null, 2))}</pre>`;
  return page("Approve agent request", `<header><div class="brand">Supersonic · Human checkpoint</div><h1>Approve an agent request</h1><span class="status">${escapeHtml(operation.state)}</span></header>
<div class="notice">${escapeHtml(notice)}</div><dl>
<dt>Agent</dt><dd>${escapeHtml(operation.agentId)}</dd><dt>Account</dt><dd>${escapeHtml(operation.accountId)}</dd>
<dt>Operation ID</dt><dd><code>${escapeHtml(operation.id)}</code></dd><dt>Method and route</dt><dd><code>${escapeHtml(operation.method)} ${escapeHtml(operation.routePath)}</code></dd>
<dt>Canonical query</dt><dd><code>${escapeHtml(operation.canonicalQuery || "(empty)")}</code></dd><dt>Consequence</dt><dd>Elevated read access. No payment is authorized.</dd>
<dt>Request fingerprint</dt><dd><code>sha256:${escapeHtml(operation.fingerprint.slice(0, 16))}…</code></dd><dt>Expires</dt><dd>${escapeHtml(new Date(operation.expiresAt).toISOString())}</dd>
${operation.errorCode ? `<dt>Outcome code</dt><dd><code>${escapeHtml(operation.errorCode)}</code></dd>` : ""}</dl>
${controls}${response}<p class="fine"><a href="/account">Back to account</a></p>`);
}

function cancelForm(operation: Operation, common: string): string {
  return `<form method="post" action="/operations/${operation.id}/cancel">${common}<button class="secondary" type="submit">Cancel request</button></form>`;
}

export function errorPage(code: string): string {
  return page("Request blocked", `<header><div class="brand">Supersonic · Fail closed</div><h1>Request blocked</h1></header><p>The protected action was not authorized.</p><p>Outcome: <code>${escapeHtml(code)}</code></p><p><a href="/account">Return to account</a></p>`);
}
