#!/usr/bin/env node
// Closed observer, not a model executor or an OS sandbox. Installation is explicit.
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { resolve, relative, isAbsolute, dirname } from 'node:path';
import { lstat, mkdir } from 'node:fs/promises';

const PROTOCOL = 'graphhelm-journey-driver/1';
const FRAME = 65536, TIMEOUT = 30000;
// A replay snapshot (expect check) may carry a real page: 32 KiB, well inside the 64 KiB frame (#434:
// a Studio Journey tab listing 21 flows is 13.8 KiB). A discover snapshot feeds a model, so it keeps
// the explore design's 6 KiB budget.
const SNAPSHOT = 32768, SNAPSHOT_DISCOVER = 6144;
// The supervisor gives each request 30 s (Rust OP_BUDGET). A screen wait that used the whole
// TIMEOUT raced that deadline and surfaced as replay.timeout instead of expectation_failed (#398).
const SCREEN_WAIT = TIMEOUT - 5000;
const fields = {
  open: ['base', 'viewport', 'allowOrigins', 'headed', 'survive', 'show'], snapshot: ['expect','discover'],
  show: ['caption', 'role', 'name'],
  act: ['kind', 'role', 'name', 'text', 'secretEnv', 'locator'],
  capture: ['path', 'maskSecrets'], close: [],
};
const roles = new Set(['dialog','alertdialog','banner','complementary','contentinfo','form','main','navigation','region','search','heading','button','checkbox','combobox','link','menuitem','menuitemcheckbox','menuitemradio','option','radio','searchbox','slider','spinbutton','switch','tab','textbox','treeitem']);
const landmarks = new Set(['banner','complementary','contentinfo','form','main','navigation','region','search']);
const secrets = Object.entries(process.env).filter(([key]) => /^GRAPHHELM_SECRET_[A-Za-z0-9_]+$/.test(key));
let browser, context, page, baseOrigin, allowed = new Set(), hostRefused = false, networkFailure;
let requestId = 0, opened = false, closed = false, headed = false, survive = false, showing = false;
// A headed (live) session survives an observation failure so the owner sees where the journey
// broke (#398); so does a healing replay's session, which repairs the broken edge in place
// (#356). Protocol, privacy and host failures still end it.
const SURVIVABLE = new Set(['driver.locator_missing','driver.locator_ambiguous','driver.expectation_failed','driver.action_failed','driver.timeout']);
const secretInputs = [];
const args = process.argv.slice(2);
const project = args[0] === '--project' && args[2] === '--output-dir' && args.length === 4 ? resolve(args[1]) : null;
const output = project ? resolve(args[3]) : null;
function fail(code) { throw Object.assign(new Error(code), { code }); }
function plain(v) { return v !== null && typeof v === 'object' && !Array.isArray(v); }
function exactKeys(value, required, optional = []) {
  return plain(value) && required.every(key => Object.hasOwn(value,key)) && Object.keys(value).every(key => required.includes(key) || optional.includes(key));
}
function string(v, max = 256, empty = false) { return typeof v === 'string' && (empty || v.length > 0) && Buffer.byteLength(v) <= max; }
function redacted(text) {
  for (const [name,value] of secrets.toSorted((a,b) => b[1].length-a[1].length)) if (value) text = text.replaceAll(value, `«secret:${name.slice(17)}»`);
  return text;
}
function clean(value) {
  const bytes = JSON.stringify(value);
  if (secrets.some(([,v]) => v && bytes.includes(v))) fail('driver.redaction_failed');
  return value;
}
function url(raw, local = false, originOnly = false) {
  if (!string(raw, 2048) || /[\\\s]/u.test(raw) || /%/.test(raw.split('/')[2] || '')) fail('driver.host_refused');
  let parsed; try { parsed = new URL(raw); } catch { fail('driver.host_refused'); }
  if (!['http:','https:'].includes(parsed.protocol) || parsed.username || parsed.password) fail('driver.host_refused');
  const host = parsed.hostname.toLowerCase();
  if (local && !(host === 'localhost' || host === '127.0.0.1' || host === '[::1]' || host.endsWith('.localhost') || host.endsWith('.test'))) fail('driver.host_refused');
  if (originOnly && (parsed.pathname !== '/' || parsed.search || parsed.hash || raw.replace(/\/$/,'') !== parsed.origin)) fail('driver.host_refused');
  return parsed;
}
function pair(v, empty = false) { return exactKeys(v,['role','name']) && string(v.role,64) && string(v.name,256,empty); }
function validate(r) {
  if (!plain(r) || r.protocol !== PROTOCOL || !Number.isSafeInteger(r.requestId) || r.requestId !== requestId + 1 || !Object.hasOwn(fields,r.op)) fail('driver.protocol_invalid');
  if (!exactKeys(r,['protocol','requestId','op'],fields[r.op])) fail('driver.protocol_invalid');
  if (r.op === 'open') {
    if (Object.hasOwn(r,'headed') && typeof r.headed !== 'boolean') fail('driver.protocol_invalid');
    if (Object.hasOwn(r,'survive') && typeof r.survive !== 'boolean') fail('driver.protocol_invalid');
    // #491: `show` (a watch) is a visible, maximized window for a person; never a headless one.
    if (Object.hasOwn(r,'show') && (typeof r.show !== 'boolean' || (r.show && r.headed !== true))) fail('driver.protocol_invalid');
    if (!exactKeys(r.viewport,['width','height']) || ![r.viewport.width,r.viewport.height].every(n => Number.isInteger(n) && n > 0 && n <= 16384) || r.viewport.width*r.viewport.height > 16777216 || !Array.isArray(r.allowOrigins) || r.allowOrigins.length > 32) fail('driver.protocol_invalid');
    url(r.base,true); for (const origin of r.allowOrigins) url(origin,false,true);
  } else if (r.op === 'snapshot') {
    if (!Array.isArray(r.expect) || r.expect.length > 8 || !r.expect.every(v => pair(v))) fail('driver.protocol_invalid');
    if (Object.hasOwn(r,'discover') && typeof r.discover !== 'boolean') fail('driver.protocol_invalid');
  } else if (r.op === 'act') {
    if (!string(r.kind,64) || !string(r.role,64) || !string(r.name,256)) fail('driver.protocol_invalid');
    if (!['activate','submit','enter_text','navigate','wait_for','inspect'].includes(r.kind)) fail('driver.unsupported_act');
    if (r.kind === 'enter_text') {
      if (Object.hasOwn(r,'text') === Object.hasOwn(r,'secretEnv') || (Object.hasOwn(r,'text') && !string(r.text,2048,true)) || (Object.hasOwn(r,'secretEnv') && (typeof r.secretEnv !== 'string' || !/^GRAPHHELM_SECRET_[A-Za-z0-9_]+$/.test(r.secretEnv)))) fail('driver.protocol_invalid');
      if (Object.hasOwn(r,'secretEnv') && !process.env[r.secretEnv]) fail('driver.secret_missing');
      if (Object.hasOwn(r,'text') && secrets.some(([,v]) => v && v === r.text)) fail('driver.secret_literal');
    } else if (Object.hasOwn(r,'text') || Object.hasOwn(r,'secretEnv')) fail('driver.protocol_invalid');
    if (Object.hasOwn(r,'locator')) {
      const l = r.locator;
      if (!exactKeys(l,['role','name','exact','testId','context','nth']) || l.role !== r.role || l.name !== r.name || l.exact !== true || l.nth !== null || !(l.testId === null || string(l.testId,128)) || !(l.context === null || string(l.context,512))) fail('driver.protocol_invalid');
    }
  } else if (r.op === 'show') {
    if (!showing || !string(r.caption,200) || !string(r.role,64) || !string(r.name,256)) fail('driver.protocol_invalid');
  } else if (r.op === 'capture') {
    if (!string(r.path,512) || r.maskSecrets !== true || isAbsolute(r.path) || r.path.includes('\\') || r.path.split('/').some(p => !p || p === '.' || p === '..') || !/\.png$/.test(r.path)) fail('driver.protocol_invalid');
  }
  // Refuse a raw secret in any approved name, origin, cache locator or literal before I/O.
  if (secrets.some(([,v]) => v && JSON.stringify(r).includes(v))) fail('driver.secret_literal');
  requestId = r.requestId;
}
async function release() {
  try { if (context) await context.close(); }
  finally {
    context = undefined; page = undefined;
    if (browser) await browser.close();
    browser = undefined;
  }
}
function checkHost() {
  if (networkFailure) fail(networkFailure);
  if (hostRefused || !page || url(page.url(),true).origin !== baseOrigin) fail('driver.host_refused');
}
async function unique(locator, missing = 'driver.locator_missing') {
  const count = await locator.count();
  if (count > 1) fail('driver.locator_ambiguous');
  if (count === 0) fail(missing);
  if (!(await locator.isVisible())) fail(missing);
  return locator;
}
async function locate(r) {
  const waiting=r.kind==='wait_for';
  const global = page.getByRole(r.role,{ name:r.name,exact:true,includeHidden:waiting });
  const select=async target=>{
    if(waiting) await target.waitFor({state:'visible',timeout:TIMEOUT});
    return unique(target);
  };
  if (r.locator?.testId !== null && r.locator?.testId !== undefined) {
    const byId = page.getByTestId(r.locator.testId), count = await byId.count();
    if (count > 1) fail('driver.locator_ambiguous');
    if (count === 1) return select(byId.and(global));
  }
  if (r.locator?.context) {
    const match = /^([a-z]+) ("(?:[^"\\]|\\.)*")$/.exec(r.locator.context);
    if (!match || !landmarks.has(match[1])) fail('driver.protocol_invalid');
    let name; try { name = JSON.parse(match[2]); } catch { fail('driver.protocol_invalid'); }
    const landmark = page.getByRole(match[1],{name,exact:true}), count = await landmark.count();
    if (count > 1) fail('driver.locator_ambiguous');
    if (count === 1) {
      const within = landmark.getByRole(r.role,{name:r.name,exact:true,includeHidden:waiting}), hits = await within.count();
      if (hits > 1) fail('driver.locator_ambiguous');
      if (hits === 1) return select(within);
    }
  }
  return select(global);
}
async function observedLocator(target,r) {
  const testId = await target.getAttribute('data-testid');
  const ancestry = await target.evaluate(el => {
    const implied = {MAIN:'main',NAV:'navigation',ASIDE:'complementary',HEADER:'banner',FOOTER:'contentinfo',FORM:'form'};
    for (let parent=el.parentElement; parent; parent=parent.parentElement) {
      const role=parent.getAttribute('role') || implied[parent.tagName];
      if (!['banner','complementary','contentinfo','form','main','navigation','region','search'].includes(role)) continue;
      const labelled=parent.getAttribute('aria-labelledby');
      const name=parent.getAttribute('aria-label') || (labelled ? labelled.split(/\s+/).map(id=>document.getElementById(id)?.textContent||'').join(' ').trim() : '');
      return `${role} ${JSON.stringify(name)}`;
    }
    return null;
  });
  return clean({role:r.role,name:r.name,exact:true,testId:testId && Buffer.byteLength(testId)<=128 ? redacted(testId) : null,context:ancestry && Buffer.byteLength(ancestry)<=512 ? redacted(ancestry) : null,nth:null});
}
/** A page over the model's budget as its outline (#356): landmarks, headings and controls in page
 * order, without their text, cut at whole lines and ending with a note saying so. */
function summarized(aria, budget) {
  if (Buffer.byteLength(aria) <= budget) return aria;
  const lines = aria.split('\n');
  const outline = lines.filter(line => { const m = /^\s*- ([a-z]+)/.exec(line); return m !== null && roles.has(m[1]); })
    .map(line => line.replace(/:\s.*$/, '').replace(/:$/, ''));
  const kept = [];
  let size = 0;
  for (const line of outline) {
    const next = Buffer.byteLength(line) + 1;
    if (size + next > budget - 96) break;
    kept.push(line); size += next;
  }
  kept.push(`- note "page summarized: ${kept.length} of ${lines.length} lines, text left out"`);
  return kept.join('\n');
}
function skeleton(aria) {
  const controlMap = new Map(), listCounts = [], listStack = [];
  for (const line of aria.split('\n')) {
    const match = /^(\s*)- ([a-z]+)(?: ("(?:[^"\\]|\\.)*"))?/.exec(line);
    if (!match) continue;
    const indent = match[1].length, role = match[2];
    while (listStack.length && indent <= listStack.at(-1).indent) listCounts.push(listStack.pop().count);
    if (role === 'list') listStack.push({indent,count:0});
    if (role === 'listitem' && listStack.length) listStack.at(-1).count++;
    if (!roles.has(role)) continue;
    let name=''; try { name = match[3] ? JSON.parse(match[3]) : ''; } catch { fail('driver.protocol_invalid'); }
    name=redacted(name).replace(/\d+/g,'#').replace(/\s+/g,' ').trim();
    if (!string(name,256,true)) fail('driver.snapshot_too_large');
    controlMap.set(JSON.stringify([role,name]),{role,name});
  }
  while (listStack.length) listCounts.push(listStack.pop().count);
  const controls=[...controlMap.values()].sort((a,b) => a.role<b.role ? -1 : a.role>b.role ? 1 : a.name<b.name ? -1 : a.name>b.name ? 1 : 0);
  if (controls.length>128) fail('driver.snapshot_too_large');
  const lists=listCounts.map(n => n <= 1 ? '1' : n <= 5 ? '2–5' : '6+').sort();
  const fingerprint='sha256:'+createHash('sha256').update(JSON.stringify({controls,lists})).digest('hex');
  return {controls,fingerprint};
}
async function run(r) {
  if (r.op === 'open') {
    if (opened) fail('driver.protocol_invalid');
    headed = r.headed === true; survive = headed || r.survive === true; showing = r.show === true;
    const parsed=url(r.base,true); baseOrigin=parsed.origin;
    allowed = new Set([baseOrigin,...r.allowOrigins.map(o=>url(o,false,true).origin)]);
    let chromium;
    try { chromium = createRequire(resolve(project,'package.json'))('@playwright/test').chromium; } catch { fail('driver.observer_missing'); }
    try {
      // #491: a watch opens maximized and the page takes the real window size (viewport: null),
      // so the owner can read it; every other session keeps its fixed, reproducible viewport.
      const launchArgs=['--host-resolver-rules=MAP *.test 127.0.0.1,MAP *.localhost 127.0.0.1,EXCLUDE localhost'];
      if (r.show === true) launchArgs.push('--start-maximized');
      browser=await chromium.launch({headless:r.headed !== true,timeout:TIMEOUT,args:launchArgs});
      context=await browser.newContext({viewport:r.show === true ? null : r.viewport,serviceWorkers:'block',acceptDownloads:false});
      context.setDefaultTimeout(TIMEOUT); context.setDefaultNavigationTimeout(TIMEOUT);
      if (typeof context.routeWebSocket !== 'function') fail('driver.observer_missing');
      await context.route('**/*',async route => {
        const request=route.request(); let parsed;
        try { parsed=url(request.url()); } catch { hostRefused=true; await route.abort(); return; }
        if (!allowed.has(parsed.origin) || (request.isNavigationRequest() && parsed.origin !== baseOrigin)) { hostRefused=true; await route.abort(); return; }
        // continue() follows redirects without invoking the route again. Inspect the
        // response with redirect following disabled before releasing it to Chromium.
        try {
          // APIRequestContext does not use Chromium's resolver rules. Pin its
          // approved aliases too, preserving the virtual host header.
          const transport=new URL(request.url());
          if (transport.hostname.endsWith('.test') || transport.hostname.endsWith('.localhost')) transport.hostname='127.0.0.1';
          const response=await route.fetch({url:transport.href,headers:{...request.headers(),host:parsed.host},maxRedirects:0,timeout:TIMEOUT});
          const location=response.headers().location;
          if (response.status()>=300 && response.status()<400 && location) {
            // Playwright bypasses route handlers for the remainder of a redirect
            // chain, including fulfilled 302s. No chain is released to Chromium:
            // checking only its first Location would allow a local-to-remote hop.
            hostRefused=true;await route.abort();return;
          }
          await route.fulfill({response});
        } catch(err) {
          networkFailure=err.name==='TimeoutError' ? 'driver.timeout' : 'driver.action_failed';
          await route.abort().catch(()=>{});
        }
      });
      await context.routeWebSocket('**/*',socket => {
        let parsed; try { parsed=new URL(socket.url()); } catch { hostRefused=true; socket.close(); return; }
        const origin=parsed.origin.replace(/^ws:/,'http:').replace(/^wss:/,'https:');
        if (!allowed.has(origin) || parsed.username || parsed.password) {hostRefused=true;socket.close();return;}
        socket.connectToServer();
      });
      page=await context.newPage();
      await page.goto(r.base,{waitUntil:'domcontentloaded',timeout:TIMEOUT});
      opened=true; checkHost();
      return {url:redacted(page.url())};
    } catch (err) { if (err.code) throw err; if (networkFailure) fail(networkFailure); if (hostRefused) fail('driver.host_refused'); if (err.name==='TimeoutError') fail('driver.timeout'); fail('driver.observer_missing'); }
  }
  if (r.op === 'close') { await release(); closed=true; return {closed:true}; }
  if (!opened || closed) fail('driver.protocol_invalid');
  checkHost();
  if (r.op === 'snapshot') {
    for (const expected of r.expect) {
      // A screen renders after its document loads; wait for the control, then require it unique.
      const target=page.getByRole(expected.role,{name:expected.name,exact:true});
      try { await target.first().waitFor({state:'visible',timeout:SCREEN_WAIT}); } catch (err) { if (err.code) throw err; fail('driver.expectation_failed'); }
      await unique(target,'driver.expectation_failed');
    }
    const mainAria=redacted(await page.locator('body').ariaSnapshot({timeout:TIMEOUT}));
    let ariaYaml=mainAria;
    const expectations=[];
    if (r.discover) {
      const frames=page.frames();
      if (frames.length>65) fail('driver.snapshot_too_large');
      // Browser frame handles reach isolated/nested documents, unlike page text
      // locators. Redact their text before any model-capable caller can receive it.
      for(const frame of frames) if(frame!==page.mainFrame()) {
        try { ariaYaml+='\n'+redacted(await frame.locator('body').ariaSnapshot({timeout:TIMEOUT})); }
        catch { fail('driver.redaction_failed'); }
        if(Buffer.byteLength(ariaYaml)>SNAPSHOT) fail('driver.snapshot_too_large');
      }
      const candidates=new Map();
      for(const match of mainAria.matchAll(/^\s*- ([a-z]+) ("(?:[^"\\]|\\.)*")/gm)) {
        let name;try{name=JSON.parse(match[2])}catch{fail('driver.protocol_invalid')}
        if(roles.has(match[1])&&string(name,256)&&!name.includes('«secret:')) candidates.set(JSON.stringify([match[1],name]),{role:match[1],name});
      }
      const rank=role=>role==='heading'?0:role==='button'?1:role==='link'?2:3;
      const ordered=[...candidates.values()].sort((a,b)=>rank(a.role)-rank(b.role)||(a.role<b.role?-1:a.role>b.role?1:0)||(a.name<b.name?-1:a.name>b.name?1:0));
      for(const candidate of ordered) {
        const locator=page.getByRole(candidate.role,{name:candidate.name,exact:true});
        if(await locator.count()===1 && await locator.isVisible()) expectations.push(candidate);
        if(expectations.length===8) break;
      }
    }
    if (Buffer.byteLength(ariaYaml)>SNAPSHOT) fail('driver.snapshot_too_large');
    checkHost();
    // Identity reads the whole page; the model gets the page within its budget (#356).
    return clean({url:redacted(page.url()),ariaYaml:r.discover?summarized(ariaYaml,SNAPSHOT_DISCOVER):ariaYaml,...skeleton(ariaYaml),...(r.discover?{expectations}:{})});
  }
  if (r.op === 'act') {
    const target=await locate(r), locator=await observedLocator(target,r);
    if (r.kind === 'enter_text') {
      const value = r.secretEnv ? process.env[r.secretEnv] : r.text;
      await target.fill(value,{timeout:TIMEOUT});
      // Values of all filled inputs are masked, including approved literals.
      secretInputs.push(target);
    } else if (['activate','submit','navigate'].includes(r.kind)) await target.click({timeout:TIMEOUT});
    else await target.waitFor({state:'visible',timeout:TIMEOUT});
    checkHost();
    return {url:redacted(page.url()),locator};
  }
  if (r.op === 'show') {
    // #491: name the coming step in a caption bar and outline the control it will touch. Both are
    // hidden from the accessibility tree (aria-hidden; outline is style only), so no snapshot,
    // expectation or locator sees them. A control that cannot be found is not an error here:
    // the act that follows reports it.
    let shown=false;
    try {
      await page.evaluate((caption)=>{
        let bar=document.getElementById('graphhelm-watch-caption');
        if(!bar){
          bar=document.createElement('div');
          bar.id='graphhelm-watch-caption';
          bar.setAttribute('aria-hidden','true');
          bar.style.cssText='position:fixed;top:0;left:0;right:0;z-index:2147483647;padding:8px 16px;font:600 15px/1.4 system-ui,sans-serif;color:#fff;background:rgba(20,20,24,.92);border-bottom:3px solid #ff7a1a;pointer-events:none';
          document.documentElement.appendChild(bar);
        }
        bar.textContent=caption;
      }, r.caption);
      const target=page.getByRole(r.role,{name:r.name,exact:true}).first();
      if (await target.count()>0) {
        await target.scrollIntoViewIfNeeded({timeout:2000}).catch(()=>{});
        await target.evaluate(el=>{
          el.style.outline='3px solid #ff7a1a'; el.style.outlineOffset='3px';
          setTimeout(()=>{ el.style.outline=''; el.style.outlineOffset=''; },1500);
        });
        shown=true;
      }
    } catch { shown=false; }
    return {shown};
  }
  if (r.op === 'capture') {
    const target=resolve(output,r.path);
    if (relative(output,target).startsWith('..') || !target.startsWith(output)) fail('driver.protocol_invalid');
    await mkdir(output,{recursive:true});
    for (let cursor=dirname(target);;cursor=dirname(cursor)) {
      const info=await lstat(cursor);
      if (info.isSymbolicLink() || !info.isDirectory()) fail('driver.capture_refused');
      if (cursor===output) break;
      if (cursor===dirname(cursor)) fail('driver.capture_refused');
    }
    try { await lstat(target); fail('driver.capture_refused'); } catch(err) {if(err.code!=='ENOENT') throw err;}
    // Page text locators do not cross frame boundaries, but screenshots do.
    // Cover entire frame elements: same-origin, isolated/cross-origin and their
    // nested content are opaque without trusting frame DOM or echo rendering.
    const mask=[...secretInputs,page.locator('input,textarea,[contenteditable="true"],iframe,frame')];
    // Closed shadow roots and nested documents may hide frame hosts from page
    // selectors. Refuse rather than claim their entire rendered area is covered.
    if (await page.locator('iframe,frame').count() !== page.frames().length-1) fail('driver.capture_refused');
    for (const [,value] of secrets) if (value) mask.push(page.getByText(value,{exact:false}));
    await page.screenshot({path:target,fullPage:false,mask,maskColor:'#FF00FF',timeout:TIMEOUT});
    if (await page.locator('iframe,frame').count() !== page.frames().length-1) fail('driver.capture_refused');
    checkHost();
    return {path:r.path,width:page.viewportSize().width,height:page.viewportSize().height,masked:true};
  }
  fail('driver.protocol_invalid');
}
async function reply(r) {
  let result;
  try {
    validate(r);
    if (!project) fail('driver.protocol_invalid');
    result=clean({protocol:PROTOCOL,requestId:r.requestId,ok:true,result:await run(r)});
  } catch(err) {
    const code=err.code || (err.name==='TimeoutError' ? 'driver.timeout' : 'driver.action_failed');
    result={protocol:PROTOCOL,requestId:Number.isSafeInteger(r?.requestId) ? r.requestId : requestId+1,ok:false,code,path:'/'};
  }
  let line=JSON.stringify(result)+'\n';
  if (Buffer.byteLength(line)>FRAME) line=JSON.stringify({protocol:PROTOCOL,requestId:result.requestId,ok:false,code:'driver.frame_too_large',path:'/'})+'\n';
  await new Promise(resolveWrite => process.stdout.write(line,resolveWrite));
  if (!result.ok && survive && opened && SURVIVABLE.has(result.code)) return true;
  if (!result.ok) { await release().catch(()=>{}); process.exitCode=1; return false; }
  return !closed;
}
let pending=Buffer.alloc(0), keep=true;
try {
  for await (const chunk of process.stdin) {
    pending=Buffer.concat([pending,chunk]);
    while (keep) {
      const end=pending.indexOf(10);
      if (end<0) {
        if(pending.length>FRAME) {
          await new Promise(done=>process.stdout.write(JSON.stringify({protocol:PROTOCOL,requestId:requestId+1,ok:false,code:'driver.frame_too_large',path:'/'})+'\n',done));
          process.exitCode=1; keep=false;
        }
        break;
      }
      if (end>FRAME) { await new Promise(done=>process.stdout.write(JSON.stringify({protocol:PROTOCOL,requestId:requestId+1,ok:false,code:'driver.frame_too_large',path:'/'})+'\n',done)); process.exitCode=1; keep=false; break; }
      const frame=pending.subarray(0,end); pending=pending.subarray(end+1);
      let r; try { r=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(frame)); } catch { r=null; }
      keep=await reply(r);
    }
    if (!keep) break;
  }
  if (keep && (pending.length || !closed)) {
    const code=pending.length>FRAME ? 'driver.frame_too_large' : 'driver.protocol_invalid';
    await new Promise(done=>process.stdout.write(JSON.stringify({protocol:PROTOCOL,requestId:requestId+1,ok:false,code,path:'/'})+'\n',done)); process.exitCode=1;
  }
} catch { process.exitCode=1; }
finally { await release().catch(()=>{process.exitCode=1;}); }
