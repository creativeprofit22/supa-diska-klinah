// Headless Edge, isolated profile, fixture page only. No desktop capture or native IPC.
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import assert from "node:assert/strict";
import { analyzerPrelude, runAnalyzerRoute } from "./fixtures/analyzer-route.mjs";

import { largeFilesPrelude, runLargeFilesRoute } from "./fixtures/large-files-route.mjs";
import { remainingPrelude, runRemainingRoutes } from "./fixtures/remaining-storage-routes.mjs";
const remaining = process.argv.includes("--remaining");
const largeFiles = process.argv.includes("--large-files");
const analyzer = process.argv.includes("--analyzer");
const port = Number(process.env.STORAGE_UI_PORT ?? 1521);
assert.ok(port === 1520 || port === 1521, "Use only project development port 1520 or preview port 1521");
const origin = `http://127.0.0.1:${port}`;
const url = origin + (remaining ? "/#/cleaner" : largeFiles ? "/#/large-files" : analyzer ? "/#/disk-analyzer" : "/scripts/fixtures/storage.html");
const nativeFixture = largeFiles ? JSON.parse(await readFile(".gg/smoke-artifacts/large-files-native.json", "utf8")) : analyzer ? JSON.parse(await readFile(".gg/smoke-artifacts/analyzer-native.json", "utf8")) : null;
const profile = await mkdtemp(join(tmpdir(), "storage-ui-"));
const artifacts = resolve(remaining ? ".gg/smoke-artifacts/remaining-storage-routes" : largeFiles ? ".gg/smoke-artifacts/large-files-route" : analyzer ? ".gg/smoke-artifacts/analyzer-route" : ".gg/smoke-artifacts/storage-frontend");
await mkdir(artifacts, { recursive: true });
const edge = join(process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)", "Microsoft/Edge/Application/msedge.exe");
const browser = spawn(edge, ["--headless=new", "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--remote-debugging-address=127.0.0.1", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"], { stdio: "ignore", windowsHide: true });
let socket;
let sequence = 0;
const pending = new Map();
const failures = [];
const watchdog = setTimeout(() => browser.kill(), 90000);
async function until(test, label, timeout = 10000) {
  const deadline = Date.now() + timeout;
  do { if (await test()) return; await delay(50); } while (Date.now() < deadline);
  throw new Error(`Timed out: ${label}`);
}
function command(method, params = {}) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {pending.delete(id); reject(new Error(`CDP timeout: ${method}`));}, method === "Page.navigate" ? 20000 : 5000);
    pending.set(id, {resolve, reject, timeout});
    socket.send(JSON.stringify({id, method, params}));
  });
}
async function evaluate(expression) {
  const result = await command("Runtime.evaluate", {expression, returnByValue:true, awaitPromise:true});
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? "Fixture expression failed");
  return result.result.value;
}
async function press(key, windowsVirtualKeyCode, modifiers = 0) {
  const text = key === "Enter" ? "\r" : key === " " ? " " : undefined;
  await command("Input.dispatchKeyEvent", {type:text ? "keyDown" : "rawKeyDown", key, windowsVirtualKeyCode, modifiers, text});
  await command("Input.dispatchKeyEvent", {type:"keyUp", key, windowsVirtualKeyCode, modifiers});
}
async function tabTo(text, limit = 24) {
  for(let i=0;i<limit;i++) {
    if(await evaluate(`document.activeElement?.textContent === ${JSON.stringify(text)}`)) return;
    await press("Tab",9);
  }
  throw new Error(`Keyboard could not reach ${text}`);
}
async function ready() { await until(()=>evaluate(`!!document.querySelector('[aria-label="Fixture scan results"]')`),"scan results"); }
async function shot(name) {
  const result = await command("Page.captureScreenshot", {format:"png", captureBeyondViewport:false});
  await writeFile(join(artifacts, `${name}.png`), Buffer.from(result.data,"base64"));
}
async function noOverflow() {
  const layout = await evaluate(`({client:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth,overflow:[...document.querySelectorAll('body *')].filter(e=>e.getBoundingClientRect().right > document.documentElement.clientWidth + 0.5).slice(0,8).map(e=>({tag:e.tagName,cls:e.className,width:e.getBoundingClientRect().width,min:getComputedStyle(e).minWidth}))})`);
  assert.ok(layout.scroll <= layout.client, `horizontal overflow: ${JSON.stringify(layout)}`);
}
try {
  let port;
  await until(async()=>{try {port=Number((await readFile(join(profile,"DevToolsActivePort"),"utf8")).split("\n")[0]);return port>0;}catch{return false;}},"headless browser startup");
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const target = targets.find(t=>t.type === "page");
  assert.ok(target?.webSocketDebuggerUrl);
  socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.addEventListener("open",resolve,{once:true});socket.addEventListener("error",reject,{once:true});});
  socket.addEventListener("message", event=>{
    const data=JSON.parse(event.data);
    if(data.method === "Log.entryAdded" && data.params.entry.level === "error") failures.push(data.params.entry.text);
    if(data.method === "Runtime.exceptionThrown") failures.push(data.params.exceptionDetails.exception?.description ?? data.params.exceptionDetails.text);
    if(data.id && pending.has(data.id)) {
      const request=pending.get(data.id);pending.delete(data.id);clearTimeout(request.timeout);
      if(data.error) request.reject(new Error(JSON.stringify(data.error))); else request.resolve(data.result);
    }
  });
  await command("Runtime.enable"); await command("Page.enable"); await command("Log.enable");
  await command("Page.addScriptToEvaluateOnNewDocument", {source:remaining ? remainingPrelude() : largeFiles ? largeFilesPrelude(nativeFixture) : analyzer ? analyzerPrelude(nativeFixture) : "window.__nativeAttempts=0; window.__TAURI_INTERNALS__={invoke(){window.__nativeAttempts++;throw Error('Native IPC forbidden in browser fixture');}};"});
  await command("Emulation.setDeviceMetricsOverride",{width:1280,height:1000,deviceScaleFactor:1,mobile:false});
  await command("Page.navigate",{url});
  if (remaining) {
    const result = await runRemainingRoutes({evaluate,command,press,tabTo,until,shot,noOverflow});
    assert.deepEqual(failures,[]);
    await writeFile(join(artifacts,"result.json"),JSON.stringify({checkedAt:new Date().toISOString(),browser:(await command('Browser.getVersion')).product,...result},null,2));
    console.log('PASS: remaining actual routes, synthetic IPC replay; zero mutations. Manual evidence unverified.');
  } else if (largeFiles) {
    const result = await runLargeFilesRoute({evaluate,command,press,tabTo,until,shot,noOverflow});
    assert.deepEqual(failures,[]);
    await writeFile(join(artifacts,"result.json"),JSON.stringify({checkedAt:new Date().toISOString(),route:url,...result},null,2));
    console.log("PASS: actual large-files route, native bounded replay, keyboard review/reset/reflow; zero mutation attempts.");
  } else if (analyzer) {
    const result = await runAnalyzerRoute({evaluate,command,press,tabTo,until,shot,noOverflow});
    assert.deepEqual(failures,[]);
    await writeFile(join(artifacts,"result.json"),JSON.stringify({checkedAt:new Date().toISOString(),route:url,...result},null,2));
    console.log("PASS: actual disk-analyzer route, native fixture replay, totals/pages, desktop/mobile, keyboard, cancellation; zero forbidden calls.");
  } else {
  await until(()=>evaluate("document.querySelector('h1')?.textContent === 'Storage component fixture'"),"fixture mount", 30000);
  await tabTo("Start fixture scan"); await press("Enter",13); await ready();
  await noOverflow(); await shot("desktop-results");
  await press("Tab",9);
  assert.equal(await evaluate("document.activeElement?.type"),"checkbox");
  await press(" ",32);
  assert.equal(await evaluate("document.body.textContent.includes('1 selected')"),true);
  await tabTo("Next page"); await press("Enter",13);
  await until(()=>evaluate("document.querySelector('.cleanup-path')?.textContent.includes('Recording 5.bin')"),"next page replacement");
  assert.equal(await evaluate("document.querySelectorAll('.cleanup-selection').length"),4);
  assert.equal(await evaluate("document.body.textContent.includes('1 selected')"),true);
  await tabTo("Review: Move to Recycle Bin"); await press("Enter",13);
  await until(()=>evaluate("!!document.querySelector('dialog[open]')"),"immutable review");
  assert.equal(await evaluate("document.activeElement.textContent"),"Cancel review");
  assert.notEqual(await evaluate("getComputedStyle(document.activeElement).outlineStyle"),"none");
  await press("Tab",9,8);
  assert.equal(await evaluate("document.activeElement.textContent"),"Move to Recycle Bin");
  await press("Tab",9);
  assert.equal(await evaluate("document.activeElement.textContent"),"Cancel review");
  await shot("desktop-review-keyboard");
  await press("Escape",27);
  await until(()=>evaluate("!document.querySelector('dialog[open]')"),"Escape dismisses review");
  assert.equal(await evaluate("document.activeElement.textContent"),"Review: Move to Recycle Bin");
  await command("Emulation.setDeviceMetricsOverride",{width:320,height:850,deviceScaleFactor:1,mobile:false});
  await noOverflow(); await shot("mobile-results");
  await press("Enter",13); await until(()=>evaluate("!!document.querySelector('dialog[open]')"),"mobile review");
  await noOverflow(); await shot("mobile-review");
  await command("Emulation.setEmulatedMedia",{features:[{name:"forced-colors",value:"active"}]});
  await noOverflow(); await shot("forced-colors-review");
  await command("Emulation.setEmulatedMedia",{features:[]});
  await command("Emulation.setDeviceMetricsOverride",{width:640,height:900,deviceScaleFactor:1,mobile:false});
  await evaluate("document.documentElement.style.fontSize='200%'");
  await noOverflow(); await shot("text-200-percent-review");
  await press("Tab",9);
  assert.equal(await evaluate("document.activeElement.textContent"),"Move to Recycle Bin");
  assert.equal(await evaluate("document.activeElement.getBoundingClientRect().bottom <= innerHeight"),true,"scaled confirmation must scroll into view");
  await shot("text-200-percent-confirm-focus");
  await press("Escape",27);
  await evaluate("document.documentElement.style.fontSize=''");
  await command("Emulation.setDeviceMetricsOverride",{width:1280,height:1000,deviceScaleFactor:1,mobile:false});
  await tabTo("Start fixture scan"); await press("Enter",13); await ready();
  assert.equal(await evaluate("document.body.textContent.includes('0 selected')"),true);
  // Fixture controls only: programmatic mode changes simulate backend outcomes.
  for(const mode of ["partial","empty","expired","busy","slow"]) {
    await evaluate(`(()=>{const s=document.querySelector('select');s.value=${JSON.stringify(mode)};s.dispatchEvent(new Event('change',{bubbles:true}));})()`);
    await tabTo("Start fixture scan"); await press("Enter",13);
    if(mode === "slow") {
      await until(()=>evaluate("document.body.textContent.includes('Scanning. No files')"),"slow scan");
      await tabTo("Cancel scan"); await press("Enter",13);
      await until(()=>evaluate("document.body.textContent.includes('Scan cancelled.')"),"cancelled state");
    } else if(mode === "expired" || mode === "busy") {
      await until(()=>evaluate("!!document.querySelector('[role=alert]')"),`${mode} error`);
    } else {await ready();}
    await shot(mode);
  }
  assert.equal(await evaluate("window.__nativeAttempts"),0);
  assert.equal(await evaluate("document.querySelector('output').textContent"),"Mutation attempts: 0");
  assert.deepEqual(failures,[]);
  const version=await command("Browser.getVersion");
  await writeFile(join(artifacts,"result.json"),JSON.stringify({checkedAt:new Date().toISOString(),browser:version.product,passed:["keyboard scan, select, next page, review, focus containment, Escape and focus return","320px reflow and modal","200% text at 640px","forced colors","new scan clears selection","partial, empty, expired, busy, cancellation states"],nativeCalls:0,mutationAttempts:0,notChecked:["Narrator","real native execution, confirmation or undo","feature routes (step 3 onward)"]},null,2));
  console.log("PASS: isolated headless fixture, desktop/mobile, keyboard review/focus, page transitions, selection reset, states; zero native calls or mutations.");
  }
} catch (error) {
  console.error("Fixture diagnostics:", failures, await evaluate(`location.href === ${JSON.stringify(url)} ? JSON.stringify({text:document.body.innerText.slice(0,2000),state:document.readyState,html:document.body.innerHTML.slice(0,1000),resources:performance.getEntriesByType('resource').map(r=>r.name)}) : 'Fixture URL not reached'`).catch(()=>"Page unavailable"));
  throw error;
} finally {
  if(socket?.readyState === WebSocket.OPEN) await command("Browser.close").catch(()=>{});
  socket?.close();clearTimeout(watchdog);
  for(const request of pending.values()) {clearTimeout(request.timeout);request.reject(new Error("Browser check finished"));}
  if(browser.exitCode === null) {
    await Promise.race([new Promise(resolve=>browser.once("exit",resolve)),delay(3000)]);
    if(browser.exitCode === null) browser.kill();
  }
  // Only our randomly created, isolated profile; never the user's browser profile.
  await rm(profile,{recursive:true,force:true,maxRetries:3}).catch(()=>{});
}
