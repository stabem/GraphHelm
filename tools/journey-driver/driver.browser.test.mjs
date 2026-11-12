import { hiddenNodeChildren } from '../hidden-node-children.mjs';
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { createInterface } from 'node:readline';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { startFixture } from './fixture-server.mjs';

const project=process.env.GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT;
const driver=fileURLToPath(new URL('./driver.mjs',import.meta.url));
const protocol='graphhelm-journey-driver/1';
// All assertions use a real browser and independent static page/server/pixel oracle.
// Gap: existing observer captures never exercise semantic locators or browser origin routing.
// Cost: ~20 seconds, explicit local Playwright/Chromium, no provider/account/network install.
async function client(t, env={}) {
  assert.ok(project,'OBSERVER_MISSING: GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT');
  const output=await mkdtemp(join(tmpdir(),'gh-driver-'));
  const child=hiddenNodeChildren.spawn(process.execPath,[driver,'--project',project,'--output-dir',output],{env:{PATH:process.env.PATH,SystemRoot:process.env.SystemRoot,TEMP:process.env.TEMP,TMP:process.env.TMP,USERPROFILE:process.env.USERPROFILE,HOME:process.env.HOME,LOCALAPPDATA:process.env.LOCALAPPDATA,PLAYWRIGHT_BROWSERS_PATH:process.env.PLAYWRIGHT_BROWSERS_PATH,...env},stdio:['pipe','pipe','pipe']});
  let sequence=0, stderr='', exit;
  child.stderr.on('data',chunk=>stderr+=chunk);
  const stopped=new Promise(done=>child.on('exit',code=>{exit=code;done();}));
  const lines=createInterface({input:child.stdout})[Symbol.asyncIterator]();
  async function send(op,payload={}) {
    child.stdin.write(JSON.stringify({protocol,requestId:++sequence,op,...payload})+'\n');
    const result=await Promise.race([lines.next(),new Promise((_,reject)=>{const timer=setTimeout(()=>reject(new Error('driver reply timed out')),35000);timer.unref();})]);
    assert.equal(result.done,false,`missing reply, stderr=${stderr}`);
    const reply=JSON.parse(result.value);
    assert.equal(reply.protocol,protocol);assert.equal(reply.requestId,sequence);
    return reply;
  }
  t.after(async()=>{if(exit===undefined){child.stdin.end();await Promise.race([stopped,new Promise(done=>setTimeout(done,2000))]);if(exit===undefined)child.kill();}await stopped;assert.equal(stderr,'');await rm(output,{recursive:true,force:true});});
  return {send,output};
}
const viewport={width:1280,height:720};
async function open(c,base,allowOrigins=[],extra={}) {const r=await c.send('open',{base,viewport,allowOrigins,...extra});assert.equal(r.ok,true,JSON.stringify(r));}
async function act(c,kind,role,name,extra={}) {return c.send('act',{kind,role,name,...extra});}

// #2028: a measured rect is four viewport fractions describing a box inside the frame.
function assertRect(rect){assert.deepEqual(Object.keys(rect ?? {}).sort(),['h','w','x','y'],JSON.stringify(rect));for(const v of Object.values(rect))assert.ok(v>=0&&v<=1,JSON.stringify(rect));assert.ok(rect.w>0&&rect.h>0&&rect.x+rect.w<=1.0001&&rect.y+rect.h<=1.0001,JSON.stringify(rect));}
function withoutRect({rect,...check}){return check;}

test('snapshot wire reports each check after a missing heading and redacts diagnostics',async t=>{
  const secret='CANARY_driver_checks_721';
  const server=createServer((_,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end(`<h1>Produto conferido</h1><section role="region" aria-label="Também encontramos em"><button>Salvar item</button></section><button aria-label="${secret}">private</button>`);});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(()=>new Promise(resolve=>server.close(resolve)));
  const c=await client(t,{GRAPHHELM_SECRET_CHECKS:secret});
  await open(c,`http://127.0.0.1:${server.address().port}/`,[],{survive:true});
  const failed=await c.send('snapshot',{expect:[{role:'heading',name:'Produto encontrado'},{role:'region',name:'Também encontramos em'},{role:'button',name:'Salvar item'}]});
  assert.equal(failed.ok,false);assert.equal(failed.code,'driver.expectation_failed');
  // #2028: each seen present check carries where its element sits on the frame; an unseen one none.
  for (const check of failed.details.checks.slice(1)) assertRect(check.rect);
  assert.equal(failed.details.checks[0].rect,undefined);
  // The button sits inside the region: its box lies within the region's box.
  const [,region,button]=failed.details.checks.map(check=>check.rect);
  assert.ok(button.x>=region.x && button.y>=region.y && button.x+button.w<=region.x+region.w+1e-3 && button.y+button.h<=region.y+region.h+1e-3,JSON.stringify({region,button}));
  assert.deepEqual(failed.details.checks.map(withoutRect),[
    {role:'heading',name:'Produto encontrado',seen:false,closest:{role:'heading',name:'Produto conferido'}},
    {role:'region',name:'Também encontramos em',seen:true,closest:null},
    {role:'button',name:'Salvar item',seen:true,closest:null},
  ]);
  const success=await c.send('snapshot',{expect:[{role:'heading',name:'Produto conferido'}]});
  assert.equal(success.ok,true);assert.equal(success.result.checks[0].seen,true);
  assert.ok(!JSON.stringify(failed).includes(secret));assert.ok(!JSON.stringify(success).includes(secret));
});

// #1781: the Zuvemo defect class. A heading whose date phrase is a <span>, and a total rendered
// late with an NBSP: role+name alone stays green while the screen says the wrong thing.
test('a text expectation proves what the element says, waits for it, and names what it held',async t=>{
  const secret='CANARY_driver_text_1781';
  const server=createServer((_,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end(`<h2>Agenda de entregas <span>daqui a 1 semana</span></h2><output aria-label="Total"></output><p aria-label="Nota" role="status">token ${secret}</p><script>setTimeout(()=>{document.querySelector('output').innerHTML='Total:\\n   R$\\u00a0246,90'},400)</script>`);});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(()=>new Promise(resolve=>server.close(resolve)));
  const c=await client(t,{GRAPHHELM_SECRET_TEXT:secret});
  await open(c,`http://127.0.0.1:${server.address().port}/`,[],{survive:true});
  const ok=await c.send('snapshot',{expect:[{role:'status',name:'Total',text:'R$ 246,90'},{role:'heading',name:'Agenda de entregas daqui a 1 semana',text:'daqui a  1 semana'}]});
  assert.equal(ok.ok,true,JSON.stringify(ok));
  assertRect(ok.result.checks[0].rect);assertRect(ok.result.checks[1].rect);
  assert.deepEqual(withoutRect(ok.result.checks[0]),{role:'status',name:'Total',text:'R$ 246,90',actual:'Total: R$ 246,90',seen:true,closest:null});
  const wrong=await c.send('snapshot',{expect:[{role:'heading',name:'Agenda de entregas daqui a 1 semana',text:'daqui a 2 dias'},{role:'status',name:'Nota',text:'nada'}]});
  assert.equal(wrong.ok,false);assert.equal(wrong.code,'driver.expectation_failed');
  assert.deepEqual(wrong.details.checks[0],{role:'heading',name:'Agenda de entregas daqui a 1 semana',text:'daqui a 2 dias',actual:'Agenda de entregas daqui a 1 semana',seen:false,closest:null});
  assert.equal(wrong.details.checks[1].actual,'token «secret:TEXT»');
  assert.ok(!JSON.stringify(wrong).includes(secret));
  const both=await c.send('snapshot',{expect:[{role:'status',name:'Total',text:'R$',absent:true}]});
  assert.equal(both.code,'driver.protocol_invalid');
});

// #2050: the three waits (present, text, absent) share one screen deadline. Before, a text that
// never matched, a missing element and a banner that never left each waited their own 25 s, so the
// screen ended in replay.timeout with nothing seen. Now it fails inside the budget and names all.
// Cost: one real browser page, ~26 s (the shared deadline itself).
test('a never-matching text, a missing element and a lingering absent share one screen deadline and all get named',async t=>{
  const server=createServer((_,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end('<h2>Agenda para daqui a 1 semana</h2><div role="alert" aria-label="Aviso">ainda aqui</div>');});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(()=>new Promise(resolve=>server.close(resolve)));
  const c=await client(t);
  await open(c,`http://127.0.0.1:${server.address().port}/`);
  const r=await c.send('snapshot',{expect:[
    {role:'heading',name:'Agenda para daqui a 1 semana',text:'daqui a 2 dias'},
    {role:'button',name:'Nao existe'},
    {role:'alert',name:'Aviso',absent:true}]});
  assert.equal(r.ok,false,JSON.stringify(r));assert.equal(r.code,'driver.expectation_failed');
  const [text,missing,lingering]=r.details.checks;
  assert.equal(text.actual,'Agenda para daqui a 1 semana');assert.equal(text.text,'daqui a 2 dias');assert.equal(text.seen,false);
  assert.equal(missing.name,'Nao existe');assert.equal(missing.seen,false);
  assert.equal(lingering.name,'Aviso');
});

test('exact names, ambiguity, contextual cache and actual supported actions',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);await open(c,f.base+'/controls');
  assert.equal((await act(c,'activate','button','Save')).ok,true);
  assert.equal((await c.send('snapshot',{expect:[{role:'heading',name:'Saved'}]})).ok,true);
  const locator={role:'button',name:'Save duplicate',exact:true,testId:'missing-id',context:'region "Billing"',nth:null};
  const found=await act(c,'inspect','button','Save duplicate',{locator});assert.equal(found.ok,true);assert.equal(found.result.locator.testId,'billing-save');
  assert.equal((await act(c,'wait_for','button','Save duplicate',{locator})).ok,true);
  // A wait must observe a subsequently visible control, not fail before waiting.
  assert.equal((await act(c,'activate','button','Reveal delayed')).ok,true);
  assert.equal((await act(c,'wait_for','button','Delayed')).ok,true);
  assert.equal((await act(c,'activate','button','Save duplicate')).code,'driver.locator_ambiguous');
  const missing=await client(t);await open(missing,f.base+'/controls');
  assert.equal((await missing.send('snapshot',{expect:[{role:'button',name:'Hidden'}]})).code,'driver.expectation_failed');
  const mismatch=await client(t);await open(mismatch,f.base+'/controls');
  assert.equal((await act(mismatch,'inspect','button','Save',{locator:{...locator,name:'Save',testId:'wrong-role',context:null}})).code,'driver.locator_missing');
});

test('fingerprint matches independent skeleton and number normalization',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);await open(c,f.base+'/cart');
  const snapshot=await c.send('snapshot',{expect:[{role:'heading',name:'Cart'},{role:'button',name:'Checkout'}]});
  assert.equal(snapshot.ok,true);
  // Independent reviewed skeleton includes the guest link in this actual fixture.
  assert.deepEqual(snapshot.result.controls,[{role:'button',name:'Checkout'},{role:'heading',name:'Cart'},{role:'link',name:'Guest checkout'},{role:'main',name:''}]);
  assert.equal((await act(c,'activate','button','Checkout')).ok,true);
  assert.equal((await act(c,'enter_text','textbox','Password',{text:'approved literal'})).ok,true);
  assert.equal((await act(c,'submit','button','Submit order')).ok,true);
  const done=await c.send('snapshot',{expect:[{role:'heading',name:'Order 42'}]});assert.equal(done.ok,true);
  assert.ok(done.result.controls.some(v=>v.role==='heading'&&v.name==='Order #'));
  assert.equal((await c.send('close')).ok,true);
});

// #2085: a passing check below the fold or inside a sideways-scrolling board must be on the frame
// the Runtime captures next. The frame's pixels are decoded by a second browser: the green
// checked button must be where its rect says, not only measured somewhere.
test('checked elements outside the viewport are scrolled into the captured frame',async t=>{
  const server=createServer((_,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end(`<body style="margin:0"><h1>Board</h1><div style="height:2400px"></div><div style="overflow-x:auto;width:600px"><div style="width:3000px;height:60px;position:relative"><button style="position:absolute;left:2600px;width:200px;height:60px;background:#00ff00;border:0">Far card</button></div></div><div style="height:2400px"></div></body>`);});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(()=>new Promise(resolve=>server.close(resolve)));
  const c=await client(t);
  await open(c,`http://127.0.0.1:${server.address().port}/`);
  const snap=await c.send('snapshot',{expect:[{role:'button',name:'Far card'}]});
  assert.equal(snap.ok,true,JSON.stringify(snap));
  const rect=snap.result.checks[0].rect;assertRect(rect);
  assert.equal((await c.send('capture',{path:'far.png',maskSecrets:true})).ok,true);
  const bytes=await readFile(join(c.output,'far.png'));
  const {chromium}=createRequire(join(project,'package.json'))('@playwright/test');
  const browser=await chromium.launch({headless:true});t.after(()=>browser.close());
  const page=await browser.newPage();
  const pixel=await page.evaluate(async ([data,cx,cy])=>{const image=new Image();image.src='data:image/png;base64,'+data;await image.decode();const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;const ctx=canvas.getContext('2d');ctx.drawImage(image,0,0);return Array.from(ctx.getImageData(Math.floor(cx*image.width),Math.floor(cy*image.height),1,1).data);},[bytes.toString("base64"),rect.x+rect.w*0.05,rect.y+rect.h*0.15]);
  assert.deepEqual(pixel,[0,255,0,255]);
  assert.equal((await c.send('close')).ok,true);
});

// #2085: an <img> check stands for a picture the owner can read. A 1x1 placeholder with the right
// alt is not seen; a real-size image is.
test('an img check fails on a tiny placeholder and passes on a real image',async t=>{
  const tiny='iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
  const real=`<svg xmlns='http://www.w3.org/2000/svg' width='64' height='48'><rect width='64' height='48' fill='%23336'/></svg>`;
  const server=createServer((_,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end(`<img alt="before" src="data:image/png;base64,${tiny}"><img alt="now" src="data:image/svg+xml,${real}">`);});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(()=>new Promise(resolve=>server.close(resolve)));
  const c=await client(t);
  await open(c,`http://127.0.0.1:${server.address().port}/`,[],{survive:true});
  const failed=await c.send('snapshot',{expect:[{role:'img',name:'before'},{role:'img',name:'now'}]});
  assert.equal(failed.ok,false);assert.equal(failed.code,'driver.expectation_failed');
  assert.deepEqual(failed.details.checks.map(check=>check.seen),[false,true]);
  assert.equal((await c.send('close')).ok,true);
});

test('secret fill is observed, text is redacted and independently decoded pixels are masked',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const secret='CANARY_secret_928147';
  const c=await client(t,{GRAPHHELM_SECRET_PASSWORD:secret});
  await open(c,f.base+'/checkout');
  assert.equal((await act(c,'enter_text','textbox','Password',{secretEnv:'GRAPHHELM_SECRET_PASSWORD'})).ok,true);
  const snap=await c.send('snapshot',{expect:[{role:'heading',name:'Checkout'}]});assert.equal(snap.ok,true);assert.ok(!JSON.stringify(snap).includes(secret));assert.ok(snap.result.ariaYaml.includes('«secret:PASSWORD»'));
  const capture=await c.send('capture',{path:'secret.png',maskSecrets:true});assert.equal(capture.ok,true);
  const bytes=await readFile(join(c.output,'secret.png'));assert.equal(bytes.readUInt32BE(16),1280);assert.equal(bytes.readUInt32BE(20),720);
  // A second browser decodes PNG pixels; inspecting compressed bytes does not prove masking.
  const {chromium}=createRequire(join(project,'package.json'))('@playwright/test');
  const browser=await chromium.launch({headless:true});t.after(()=>browser.close());
  const page=await browser.newPage();
  const pixels=await page.evaluate(async data=>{const image=new Image();image.src='data:image/png;base64,'+data;await image.decode();const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;const ctx=canvas.getContext('2d');ctx.drawImage(image,0,0);return [[60,100],[60,145]].map(([x,y])=>Array.from(ctx.getImageData(x,y,1,1).data));},bytes.toString('base64'));
  assert.deepEqual(pixels,[[255,0,255,255],[255,0,255,255]]);
  assert.equal((await c.send('close')).ok,true);
});

// Contract: persisted screenshots must conceal secrets echoed inside rendered frames.
// Regression/gap: main-document text masks miss both same-origin and opaque-origin frames.
// No production seam; real driver protocol plus an independent Chromium PNG decoder.
// Cost: ~4 seconds, two local browser sessions and one loopback HTTP server.
test('capture conceals same-origin and opaque-origin iframe secret echoes',async t=>{
  const secret='frame_privacy_canary_348';
  const server=createServer((req,res)=>{
    res.setHeader('Content-Type','text/html; charset=utf-8');
    const closed=req.url.includes('?shadow');
    res.end(req.url==='/frame' ? `<body style="margin:0;background:#00ff00"><p style="font:24px monospace">empty</p><script>onmessage=e=>{document.querySelector('p').textContent=e.data;parent.postMessage('echoed','*')}</script>` : `<body style="margin:0"><input aria-label="Password"><button hidden>Echo ready</button><iframe src="/frame" style="position:absolute;left:0;top:80px;border:0;width:400px;height:180px"></iframe><iframe sandbox="allow-scripts" src="/frame" style="position:absolute;left:440px;top:80px;border:0;width:400px;height:180px"></iframe><div id="host"></div><script>if(${closed}){const shadow=document.querySelector('#host').attachShadow({mode:'closed'});shadow.innerHTML='<iframe src="/frame" style="position:absolute;left:0;top:300px;border:0;width:400px;height:180px"></iframe>';window.extraFrame=shadow.querySelector('iframe')};const frames=[...document.querySelectorAll('iframe'),...(window.extraFrame?[window.extraFrame]:[])];document.querySelector('input').oninput=e=>frames.forEach(f=>f.contentWindow.postMessage(e.target.value,'*'));let echoed=0;onmessage=e=>{if(e.data==='echoed'&&++echoed===frames.length)document.querySelector('button').hidden=false}</script>`);
  });
  await new Promise(done=>server.listen(0,'127.0.0.1',done));t.after(()=>new Promise(done=>server.close(done)));
  const base=`http://127.0.0.1:${server.address().port}`;
  const {chromium}=createRequire(join(project,'package.json'))('@playwright/test');
  const browser=await chromium.launch({headless:true});t.after(()=>browser.close());
  const page=await browser.newPage({viewport});await page.goto(base+'?shadow');
  await page.getByRole('textbox',{name:'Password',exact:true}).fill(secret);
  await page.getByRole('button',{name:'Echo ready',exact:true}).waitFor();
  assert.equal(await page.frameLocator('iframe').nth(0).getByText(secret,{exact:true}).count(),1);
  assert.equal(await page.frameLocator('iframe').nth(1).getByText(secret,{exact:true}).count(),1);
  assert.equal(await page.frames()[2].evaluate(()=>{try{void parent.document;return false}catch{return true}}),true,'sandbox frame really has an isolated origin');
  const before=await page.screenshot();
  const decode=async bytes=>page.evaluate(async data=>{
    const image=new Image();image.src='data:image/png;base64,'+data;await image.decode();
    const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;
    const ctx=canvas.getContext('2d');ctx.drawImage(image,0,0);
    return [[0,100],[440,100],[0,320]].map(([left,top])=>{const pixels=ctx.getImageData(left,top,400,80).data;let ink=0,masked=0;
      for(let i=0;i<pixels.length;i+=4){if(pixels[i]<100&&pixels[i+1]<100&&pixels[i+2]<100)ink++;if(pixels[i]===255&&pixels[i+1]===0&&pixels[i+2]===255&&pixels[i+3]===255)masked++;}
      return {ink,masked};});
  },bytes.toString('base64'));
  for(const region of await decode(before)){assert.ok(region.ink>100,'positive control visibly renders the secret');assert.equal(region.masked,0);}
  const c=await client(t,{GRAPHHELM_SECRET_PASSWORD:secret});await open(c,base);
  assert.equal((await act(c,'enter_text','textbox','Password',{secretEnv:'GRAPHHELM_SECRET_PASSWORD'})).ok,true);
  assert.equal((await act(c,'wait_for','button','Echo ready')).ok,true);
  // Explore needs frame-aware text privacy before model dispatch, independently
  // of image masks. The existing replay snapshot never requested this observer.
  const discovery=await c.send('snapshot',{expect:[],discover:true});
  assert.equal(discovery.ok,true);
  assert.ok(!JSON.stringify(discovery).includes(secret));
  assert.ok(discovery.result.ariaYaml.includes('«secret:PASSWORD»'));
  assert.ok(discovery.result.expectations.some(v=>v.role==='button'&&v.name==='Echo ready'));
  const capture=await c.send('capture',{path:'frames.png',maskSecrets:true});assert.equal(capture.ok,true);assert.equal(capture.result.masked,true);
  assert.deepEqual((await decode(await readFile(join(c.output,'frames.png')))).slice(0,2),[{ink:0,masked:32000},{ink:0,masked:32000}]);
  const hidden=await client(t,{GRAPHHELM_SECRET_PASSWORD:secret});await open(hidden,base+'?shadow');
  assert.equal((await act(hidden,'enter_text','textbox','Password',{secretEnv:'GRAPHHELM_SECRET_PASSWORD'})).ok,true);
  assert.equal((await act(hidden,'wait_for','button','Echo ready')).ok,true);
  assert.equal((await hidden.send('capture',{path:'hidden.png',maskSecrets:true})).code,'driver.capture_refused');
  await assert.rejects(readFile(join(hidden.output,'hidden.png')),{code:'ENOENT'});
  assert.equal((await c.send('close')).ok,true);
});

test('context network guard blocks popup, frame, fetch, redirect, websocket and service worker',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  // Positive control proves the canary sees a real request.
  await fetch(f.canaryOrigin+'/positive');assert.equal(f.counts().canary,1);f.reset();
  for(const name of ['Fetch','Popup','Frame','WebSocket','Redirect','Redirect chain']) {
    const c=await client(t);await open(c,f.base+'/network');
    const r=await act(c,'activate','button',name);
    if(r.ok) {await new Promise(done=>setTimeout(done,100));const snap=await c.send('snapshot',{expect:[]});assert.equal(snap.code,'driver.host_refused');}
    else assert.equal(r.code,'driver.host_refused');
    assert.equal(f.counts().canary,0,name);
  }
  const sw=await client(t);await open(sw,f.base+'/network');assert.equal((await act(sw,'activate','button','Service worker')).ok,true);await new Promise(done=>setTimeout(done,100));assert.equal(f.counts().canary,0);await sw.send('close');
  const allowed=await client(t);await open(allowed,f.base+'/network',[f.canaryOrigin]);assert.equal((await act(allowed,'activate','button','Fetch')).ok,true);await new Promise(done=>setTimeout(done,100));assert.equal(f.counts().canary,1);
  assert.equal((await act(allowed,'activate','button','Popup')).code,'driver.host_refused');assert.equal(f.counts().canary,1);
  const alias=await client(t);await open(alias,f.base.replace('127.0.0.1','fixture.graphhelm.test')+'/cart');assert.equal((await alias.send('snapshot',{expect:[{role:'heading',name:'Cart'}]})).ok,true);await alias.send('close');
});

// #381: a real app renders its screen after the document loads (the Studio reads the run over
// HTTP before the team canvas exists). A screen expectation checked at the instant of
// `domcontentloaded` refused every Studio flow at its first screen. The defect: snapshot counted
// the control once instead of waiting for it. Cost: ~2 s for the late page; the absent control
// still waits the driver timeout before refusing, as a contract's failure timeout would.
test('a screen expectation waits for a control that renders after load, and still refuses one that never does',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);await open(c,f.base+'/late');
  const late=await c.send('snapshot',{expect:[{role:'heading',name:'Ready'},{role:'button',name:'Continue'}]});
  assert.equal(late.ok,true,JSON.stringify(late));
  assert.deepEqual(late.result.controls.filter(x=>x.role!=='main'),[{role:'button',name:'Continue'},{role:'heading',name:'Ready'}]);
  await c.send('close');
  const n=await client(t);await open(n,f.base+'/late');
  const never=await n.send('snapshot',{expect:[{role:'heading',name:'Never'}]});
  assert.equal(never.code,'driver.expectation_failed');
});

test('page error replies keep the event source and page URL, with bounded secret masking',async t=>{
  const secret='CANARY_page_error_context_2476';
  const server=createServer((req,res)=>{
    const page=new URL(req.url,'http://fixture.invalid').pathname;
    res.writeHead(200,{'content-type':'text/html'});
    res.end(page==='/console'
      ? `<h1>Console</h1><script>console.error(${JSON.stringify(secret)})</script>`
      : page==='/long'
        ? `<h1>Long</h1><script>console.error(${JSON.stringify('😀'.repeat(1000))})</script>`
        : `<h1>Page error</h1><script>throw new Error(${JSON.stringify(secret)})</script>`);
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));t.after(()=>new Promise(done=>server.close(done)));
  const origin=`http://127.0.0.1:${server.address().port}`;
  for(const [path,source] of [['/console','console'],['/pageerror','pageerror']]){
    const c=await client(t,{GRAPHHELM_SECRET_PAGE_ERROR:secret});
    const opened=await c.send('open',{base:`${origin}${path}?filter=recent&access_token=query_value_canary#private`,viewport,allowOrigins:[],survive:true});
    assert.equal(opened.ok,true,JSON.stringify(opened));
    assert.equal(opened.pageError.source,source,JSON.stringify(opened));
    assert.ok(Buffer.byteLength(opened.pageError.message)<=160);
    assert.ok(Buffer.byteLength(opened.pageError.url)<=512);
    assert.ok(!JSON.stringify(opened).includes(secret),'raw configured secret never leaves the driver');
    const eventUrl=new URL(opened.pageError.url);
    assert.equal(eventUrl.pathname,path,'the URL is captured from the page, not the console script location');
    assert.equal(eventUrl.searchParams.get('filter'),'recent');
    assert.equal(eventUrl.searchParams.get('access_token'),'[redacted]');
    assert.equal(eventUrl.hash,'');
    await c.send('close');
  }
  const long=await client(t);
  const longReply=await long.send('open',{base:`${origin}/long?filter=recent&access_token=query_value_canary&padding=${'x'.repeat(800)}#private`,viewport,allowOrigins:[]});
  assert.equal(longReply.ok,true,JSON.stringify(longReply));
  assert.equal(longReply.pageError.source,'console',JSON.stringify(longReply));
  assert.ok(Buffer.byteLength(longReply.pageError.message)<=160);
  assert.ok(Buffer.byteLength(longReply.pageError.url)<=512);
  assert.ok(!longReply.pageError.url.includes('#'));
  assert.ok(longReply.pageError.url.includes('access_token=%5Bredacted%5D'));
  assert.ok(!longReply.pageError.url.includes('query_value_canary'));
  await long.send('close');
});

// #356: a page whose accessibility snapshot is over the model budget (6 KiB) could not be
// discovered at all (driver.snapshot_too_large), so explore could not see busy screens such as the
// Studio's Journey tab. The model now gets the page's outline within the budget; identity still
// reads the whole page. Cost: one real browser page, seconds.
test('a discover snapshot over the model budget returns the outline and keeps whole-page identity',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);await open(c,f.base+'/big');
  const full=await c.send('snapshot',{expect:[]});
  assert.equal(full.ok,true,JSON.stringify(full).slice(0,300));
  assert.ok(Buffer.byteLength(full.result.ariaYaml)>6144,'the fixture page is over the model budget');
  const discovery=await c.send('snapshot',{expect:[],discover:true});
  assert.equal(discovery.ok,true,JSON.stringify(discovery).slice(0,300));
  const outline=discovery.result.ariaYaml;
  assert.ok(Buffer.byteLength(outline)<=6144,`outline is ${Buffer.byteLength(outline)} bytes`);
  assert.match(outline,/- heading "Big page"/);
  assert.match(outline,/page summarized: \d+ of \d+ lines, \d+ of \d+ text lines kept/);
  assert.equal(discovery.result.fingerprint,full.result.fingerprint,'identity reads the whole page');
});

// #1783: the drafter reads only this summary, so a busy page's prices and dates must reach it with
// their digits, or it can never propose a `text` check on them; the secret stays redacted, the
// budget still bounds the page, and the fingerprint still ignores numbers. Cost: one real page.
test('a discover summary keeps page text with digits, redacted and bounded, and the fingerprint ignores numbers',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t,{GRAPHHELM_SECRET_PAGE:'CANARY_priced_page_913'});await open(c,f.base+'/priced');
  const one=await c.send('snapshot',{expect:[],discover:true});
  assert.equal(one.ok,true,JSON.stringify(one).slice(0,300));
  const outline=one.result.ariaYaml;
  assert.ok(Buffer.byteLength(outline)<=6144,`summary is ${Buffer.byteLength(outline)} bytes`);
  assert.match(outline,/page summarized: /,'the fixture page is over the model budget');
  for(const seen of ['R$ 246,90','daqui a 1 semana','1 entrega','- button "Open 39"']) assert.ok(outline.includes(seen),`summary lacks ${seen}`);
  assert.ok(!JSON.stringify(one).includes('CANARY_priced_page_913'),'the secret is redacted');
  assert.ok(outline.includes('«secret:PAGE»'),'the redaction marker stays visible');
  const c2=await client(t,{GRAPHHELM_SECRET_PAGE:'CANARY_priced_page_913'});await open(c2,f.base+'/priced?v=2');
  const two=await c2.send('snapshot',{expect:[],discover:true});
  assert.equal(two.ok,true,JSON.stringify(two).slice(0,300));
  assert.ok(two.result.ariaYaml.includes('R$ 319,40'));
  assert.equal(two.result.fingerprint,one.result.fingerprint,'numbers alone are not drift');
});

// #356 calibration: an open dialog is part of a screen's identity, so the driver must report it.
// Regression: dialog was not in the driver's role set, so a modal changed nothing it saw.
// Cost: one real browser page, seconds.
test('an open dialog is reported among the controls',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);await open(c,f.base+'/dialog');
  const snap=await c.send('snapshot',{expect:[]});
  assert.equal(snap.ok,true,JSON.stringify(snap).slice(0,300));
  assert.ok(snap.result.controls.some(x=>x.role==='dialog'&&x.name==='Rename item'),JSON.stringify(snap.result.controls));
});

// #585: a flow's declared storage is the browser's starting localStorage for the base origin; it
// survives a reload and the app's own write replaces it. A declared viewport is the page's size.
// Catches storage dropped, re-seeded on every load, or written to another origin, and a viewport
// ignored. Cost: one page, ~2 seconds.
test('declared storage survives a reload until the app overwrites it, at the declared viewport',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);
  const r=await c.send('open',{base:f.base+'/storage',viewport:{width:390,height:844},allowOrigins:[],storage:[{key:'seen',value:'flow'}]});
  assert.equal(r.ok,true,JSON.stringify(r));
  const seen=async name=>{const s=await c.send('snapshot',{expect:[{role:'heading',name}]});assert.equal(s.ok,true,`${name}: ${JSON.stringify(s).slice(0,300)}`);return s.result.ariaYaml;};
  assert.match(await seen('Seen: flow'),/Width 390/);
  assert.equal((await act(c,'activate','button','Reload')).ok,true);
  await seen('Seen: flow');
  assert.equal((await act(c,'activate','button','Overwrite')).ok,true);
  await seen('Seen: app');
});

// #1782: a flow's declared clock is the page's now from the first navigation, and it still runs:
// a timer fires and Date advances with it. Catches the clock ignored, installed after the page
// loaded, or frozen (setFixedTime) so elapsed-time code never moves. Cost: one page, ~2 seconds.
test('a declared clock is the page now from the first load and timers still run',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);
  await open(c,f.base+'/clock',[],{clock:'2026-10-31T12:00:00-03:00'});
  const s=await c.send('snapshot',{expect:[{role:'heading',name:'Now 2026-10-31T15:00'},{role:'heading',name:'Later true'}]});
  assert.equal(s.ok,true,JSON.stringify(s).slice(0,600));
});

// #1777: `device: "mobile"` opens the page as the pinned phone: 390 wide, touch, a mobile user agent;
// without it the same page is a desktop. Catches the device ignored, or only the size applied.
test('the mobile device opens a 390-wide touch page with a mobile user agent',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);
  const r=await c.send('open',{base:f.base+'/device',viewport:{width:390,height:844},allowOrigins:[],device:'mobile'});
  assert.equal(r.ok,true,JSON.stringify(r));
  const s=await c.send('snapshot',{expect:[{role:'heading',name:'Width 390'},{role:'heading',name:'Touch true'},{role:'heading',name:'Mobile true'}]});
  assert.equal(s.ok,true,JSON.stringify(s).slice(0,600));
  const d=await client(t);
  await open(d,f.base+'/device');
  const w=await d.send('snapshot',{expect:[{role:'heading',name:'Width 1280'},{role:'heading',name:'Touch false'},{role:'heading',name:'Mobile false'}]});
  assert.equal(w.ok,true,JSON.stringify(w).slice(0,600));
});

// Coverage is an explicit observer: the initial page calls one named function while another stays
// untouched. This catches a missing start-before-navigation,
// an accidental zero-success artifact, or loss of function ranges/counts in serialization.
test('coverage captures called and untouched generated functions without source text',async t=>{
  const server=createServer((_req,res)=>{
    res.setHeader('Content-Type','text/html; charset=utf-8');
    res.end('<!doctype html><html><body><main><h1>Coverage</h1></main><script>function calledFunction(){return 1} function untouchedFunction(){return 2} window.coverageMarker=calledFunction()</script></body></html>');
  });
  await new Promise(done=>server.listen(0,'127.0.0.1',done));
  t.after(()=>new Promise(done=>server.close(done)));
  const c=await client(t);await open(c,`http://127.0.0.1:${server.address().port}/coverage`,[],{coverage:true});
  const result=await c.send('coverage');
  assert.equal(result.ok,true,JSON.stringify(result));
  assert.deepEqual(result.result,{path:'coverage.json',schema:'graphhelm-js-coverage/1'});
  const artifact=JSON.parse(await readFile(join(c.output,'coverage.json'),'utf8'));
  assert.equal(artifact.schema,'graphhelm-js-coverage/1');
  assert.equal(artifact.collection,'captured');
  assert.equal(artifact.navigationCount,1);
  assert.equal(artifact.negativeEvidenceEligible,true);
  assert.equal(artifact.completeness.complete,true);
  assert.ok(artifact.limitations.includes('JavaScript page coverage only'));
  assert.ok(artifact.scripts.length>0);
  const functions=artifact.scripts.flatMap(script=>script.functions);
  assert.ok(functions.some(fn=>fn.name==='calledFunction'&&fn.ranges.some(range=>range.count>0)),'a named generated function was called');
  assert.ok(functions.some(fn=>fn.name==='untouchedFunction'&&fn.ranges.every(range=>range.count===0)),'a named untouched generated function was retained');
  assert.ok(!JSON.stringify(artifact).includes('sourceContent'));
  assert.equal((await c.send('coverage')).code,'driver.protocol_invalid');
});

// Contract: coverage retains every valid range for a function while the global
// artifact bound is not exceeded. The old per-function cap silently discarded
// ranges 65+, making negative evidence incomplete for large generated functions.
// Cost: one local browser navigation, covered by the existing driver test setup.
test('coverage preserves more than 64 ranges within the global bound',async t=>{
  const branches=Array.from({length:80},(_,i)=>`if (window.coverageBranch${i}) return ${i};`).join('');
  const server=createServer((_req,res)=>{
    res.setHeader('Content-Type','text/html; charset=utf-8');
    res.end(`<script>function manyRanges(){${branches}return 80}window.coverageResult=manyRanges()</script>`);
  });
  await new Promise(done=>server.listen(0,'127.0.0.1',done));
  t.after(()=>new Promise(done=>server.close(done)));
  const c=await client(t);await open(c,`http://127.0.0.1:${server.address().port}/many-ranges`,[],{coverage:true});
  const result=await c.send('coverage');
  assert.equal(result.ok,true,JSON.stringify(result));
  const artifact=JSON.parse(await readFile(join(c.output,'coverage.json'),'utf8'));
  const fn=artifact.scripts.flatMap(script=>script.functions).find(candidate=>candidate.name==='manyRanges');
  assert.ok(fn,'generated function with many ranges was retained');
  assert.ok(fn.ranges.length>64,`expected more than 64 ranges, got ${fn.ranges.length}`);
  assert.equal(artifact.completeness.complete,true,JSON.stringify(artifact.completeness));
  assert.equal(artifact.negativeEvidenceEligible,true);
});

test('coverage keeps inline source hashes, refuses source-map attribution gaps, and marks navigation incomplete',async t=>{
  const original='function originalMapped(){return "original-source-canary"}';
  const encoded=Buffer.from(JSON.stringify({version:3,sources:['src/original.js'],sourcesContent:[original],names:[],mappings:'AAAA'})).toString('base64');
  const multi=Buffer.from(JSON.stringify({version:3,sources:['a.js','b.js'],sourcesContent:['function a(){return 1}','function b(){return 2}'],names:[],mappings:'AAAA,CCAA'})).toString('base64');
  const partial=Buffer.from(JSON.stringify({version:3,sources:['a.js','b.js'],sourcesContent:['function a(){return 1}'],names:[],mappings:''})).toString('base64');
  const invalidVlq=Buffer.from(JSON.stringify({version:3,sources:['bad.js'],sourcesContent:['function bad(){return 1}'],names:[],mappings:'!'})).toString('base64');
  const outOfBounds=Buffer.from(JSON.stringify({version:3,sources:['one.js'],sourcesContent:['function one(){return 1}'],names:[],mappings:'ACAA'})).toString('base64');
  const noAttribution=Buffer.from(JSON.stringify({version:3,sources:['none.js'],sourcesContent:['function none(){return 1}'],names:[],mappings:'A'})).toString('base64');
  const server=createServer((req,res)=>{
    res.setHeader('Content-Type','text/html; charset=utf-8');
    if(req.url==='/valid') return res.end(`<script>function generatedMapped(){return 1}generatedMapped();//# sourceMappingURL=data:application/json;base64,${encoded}</script>`);
    if(req.url==='/external') return res.end('<script>function externalMap(){return 1}externalMap();//# sourceMappingURL=https://maps.example/external-map.js</script>');
    if(req.url==='/malformed') return res.end('<script>function malformedMap(){return 1}malformedMap();//# sourceMappingURL=data:application/json;base64,not-json</script>');
    if(req.url==='/multi') return res.end(`<script>function multiMap(){return 1}multiMap();//# sourceMappingURL=data:application/json;base64,${multi}</script>`);
    if(req.url==='/partial') return res.end(`<script>function partialMap(){return 1}partialMap();//# sourceMappingURL=data:application/json;base64,${partial}</script>`);
    if(req.url==='/invalid-vlq') return res.end(`<script>function invalidVlq(){return 1}invalidVlq();//# sourceMappingURL=data:application/json;base64,${invalidVlq}</script>`);
    if(req.url==='/out-of-bounds') return res.end(`<script>function outOfBounds(){return 1}outOfBounds();//# sourceMappingURL=data:application/json;base64,${outOfBounds}</script>`);
    if(req.url==='/no-attribution') return res.end(`<script>function noAttribution(){return 1}noAttribution();//# sourceMappingURL=data:application/json;base64,${noAttribution}</script>`);
    if(req.url==='/null-map') return res.end('<script>function validBesideNull(){return 1}validBesideNull()</script><script>function nullMap(){return 1}nullMap();//# sourceMappingURL=data:application/json;base64,bnVsbA==</script>');
    if(req.url==='/nav') return res.end('<button>Navigate</button><script>document.querySelector("button").onclick=()=>location.href="/nav2"</script>');
    if(req.url==='/spa') return res.end('<button>Push state</button><script>document.querySelector("button").onclick=()=>history.pushState({},"","#next")</script>');
    res.end('<h1>Navigation target</h1>');
  });
  await new Promise(done=>server.listen(0,'127.0.0.1',done));
  t.after(()=>new Promise(done=>server.close(done)));
  const base=`http://127.0.0.1:${server.address().port}`;
  const readArtifact=async (path, expectedSources)=>{
    const c=await client(t);await open(c,base+path,[],{coverage:true});
    const reply=await c.send('coverage');assert.equal(reply.ok,true,JSON.stringify(reply));
    const artifact=JSON.parse(await readFile(join(c.output,'coverage.json'),'utf8'));
    assert.deepEqual(artifact.scripts.flatMap(s=>s.sources).map(s=>s.sha256),expectedSources);
    assert.ok(!JSON.stringify(artifact).includes('sourceContent'));
    assert.equal((await c.send('close')).ok,true);
    return artifact;
  };
  const valid=await readArtifact('/valid',[createHash('sha256').update(original).digest('hex')]);
  assert.equal(valid.negativeEvidenceEligible,true);
  for(const path of ['/external','/malformed','/partial','/invalid-vlq','/out-of-bounds','/no-attribution']) {
    const artifact=await readArtifact(path,[]);
    assert.equal(artifact.negativeEvidenceEligible,false);
    assert.equal(artifact.complete,false);
    assert.ok(!JSON.stringify(artifact).includes('external-map.js'));
  }
  const multiple=await readArtifact('/multi',[createHash('sha256').update('function a(){return 1}').digest('hex'),createHash('sha256').update('function b(){return 2}').digest('hex')]);
  assert.equal(multiple.negativeEvidenceEligible,true);
  assert.equal(multiple.complete,true);
  const nullMap=await client(t);await open(nullMap,base+'/null-map',[],{coverage:true});
  const nullReply=await nullMap.send('coverage');assert.equal(nullReply.ok,true);
  const nullArtifact=JSON.parse(await readFile(join(nullMap.output,'coverage.json'),'utf8'));
  assert.equal(nullArtifact.complete,false);
  assert.equal(nullArtifact.negativeEvidenceEligible,false);
  assert.ok(nullArtifact.scripts.some(script=>script.sources.length===0));
  assert.ok(nullArtifact.scripts.some(script=>script.functions.some(fn=>fn.name==='validBesideNull'&&fn.ranges.some(range=>range.count>0))),'valid coverage survives null map');
  assert.ok(!JSON.stringify(nullArtifact).includes('sourceMappingURL'));
  assert.equal((await nullMap.send('close')).ok,true);
  const unavailable=await client(t);await open(unavailable,base+'/valid');
  const unavailableReply=await unavailable.send('coverage');assert.equal(unavailableReply.ok,true);
  const unavailableArtifact=JSON.parse(await readFile(join(unavailable.output,'coverage.json'),'utf8'));
  assert.equal(unavailableArtifact.collection,'unavailable');
  assert.equal(unavailableArtifact.complete,false);
  assert.equal(unavailableArtifact.negativeEvidenceEligible,false);
  assert.equal((await unavailable.send('close')).ok,true);
  const navigated=await client(t);await open(navigated,base+'/nav',[],{coverage:true});
  assert.equal((await act(navigated,'activate','button','Navigate')).ok,true);
  const navReply=await navigated.send('coverage');assert.equal(navReply.ok,true);
  const navArtifact=JSON.parse(await readFile(join(navigated.output,'coverage.json'),'utf8'));
  assert.equal(navArtifact.navigationCount,2);
  assert.equal(navArtifact.negativeEvidenceEligible,false);
  assert.equal(navArtifact.complete,false);
  assert.equal((await navigated.send('close')).ok,true);
  const spa=await client(t);await open(spa,base+'/spa',[],{coverage:true});
  assert.equal((await act(spa,'activate','button','Push state')).ok,true);
  const spaReply=await spa.send('coverage');assert.equal(spaReply.ok,true);
  const spaArtifact=JSON.parse(await readFile(join(spa.output,'coverage.json'),'utf8'));
  assert.equal(spaArtifact.navigationCount,1);
  assert.equal(spaArtifact.negativeEvidenceEligible,true);
  assert.equal((await spa.send('close')).ok,true);
});

// #2721: the driver answers each request through route.fetch, an API request that carries none
// of the Sec-Fetch-* headers Chromium adds. A document navigation must still reach the server as
// the navigation it is (an app may set its session cookie only for one), while the redirect
// refusal and the host allow-list stay as they were.
test('a document navigation reaches the server with the browser\'s Sec-Fetch metadata and keeps its cookie',async t=>{
  const seen=[];
  const server=createServer((req,res)=>{
    seen.push({path:req.url,mode:req.headers['sec-fetch-mode']??'-',dest:req.headers['sec-fetch-dest']??'-',site:req.headers['sec-fetch-site']??'-',cookie:Boolean(req.headers.cookie)});
    res.setHeader('Content-Type','text/html; charset=utf-8');
    if (req.url === '/') res.setHeader('Set-Cookie','session=1; HttpOnly; Path=/');
    res.end(req.url === '/' ? '<h1>Start</h1><a href="/next">Next page</a>' : '<h1>Next</h1>');
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(()=>new Promise(resolve=>server.close(resolve)));
  const c=await client(t);
  await open(c,`http://127.0.0.1:${server.address().port}/`);
  assert.deepEqual(seen[0],{path:'/',mode:'navigate',dest:'document',site:'none',cookie:false});
  assert.equal((await act(c,'activate','link','Next page')).ok,true);
  const next=seen.find(entry=>entry.path === '/next');
  assert.ok(next,JSON.stringify(seen));
  assert.deepEqual(next,{path:'/next',mode:'navigate',dest:'document',site:'same-origin',cookie:true});
  assert.equal((await c.send('close')).ok,true);
});

// #2724: every request reaches the server with the Sec-Fetch metadata the browser would have sent.
// Site is the initiating frame's origin against the request's origin (same-origin, or cross-site,
// never the first for a different origin); mode and dest come from the resource type. The Studio
// authorizes same-origin fetch/XHR on Sec-Fetch-Site (#2615), so these must not go without it.
test('Sec-Fetch metadata: same-origin subrequests and iframes say same-origin; a cross-site subresource says cross-site',async t=>{
  const seen=[];
  const record=(req)=>seen.push({path:req.url,mode:req.headers['sec-fetch-mode']??'-',dest:req.headers['sec-fetch-dest']??'-',site:req.headers['sec-fetch-site']??'-'});
  const other=createServer((req,res)=>{record({url:'other'+req.url,headers:req.headers});res.setHeader('Content-Type','image/gif');res.end('');});
  const main=createServer((req,res)=>{
    record(req);
    const types={'/app.js':'text/javascript','/s.css':'text/css','/p.gif':'image/gif','/p2.gif':'image/gif','/api':'application/json','/post':'application/json'};
    res.setHeader('Content-Type',types[req.url]??'text/html; charset=utf-8');
    if (req.url === '/') return res.end(`<link rel="stylesheet" href="/s.css"><script src="/app.js"></script><img src="/p.gif" alt="pixel"><img src="http://other.localhost:${other.address().port}/x.gif" alt="foreign"><iframe src="/frame" title="same"></iframe><iframe srcdoc="<img src='/p2.gif'>" title="doc"></iframe><h1>Page</h1>`);
    if (req.url === '/app.js') return res.end('fetch("/api").then(r=>r.text());var x=new XMLHttpRequest();x.open("POST","/post");x.send("{}")');
    res.end(req.url === '/api' || req.url === '/post' ? '{}' : req.url.endsWith('.css') || req.url.endsWith('.gif') ? '' : '<p>frame</p>');
  });
  await Promise.all([other,main].map(server=>new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))));
  t.after(()=>Promise.all([other,main].map(server=>new Promise(resolve=>server.close(resolve)))));
  const c=await client(t);
  await open(c,`http://127.0.0.1:${main.address().port}/`,[`http://other.localhost:${other.address().port}`]);
  const wanted=['/','/s.css','/app.js','/p.gif','/api','/post','/frame','/p2.gif','other/x.gif'];
  for (let tries=0;tries<50 && !wanted.every(path=>seen.some(entry=>entry.path === path));tries++) await new Promise(resolve=>setTimeout(resolve,100));
  const of=path=>seen.find(entry=>entry.path === path);
  for (const path of wanted) assert.ok(of(path),`${path} never reached a server: ${JSON.stringify(seen)}`);
  const expect=(path,mode,dest,site)=>assert.deepEqual(of(path),{path,mode,dest,site},path);
  expect('/','navigate','document','none');
  expect('/frame','navigate','iframe','same-origin');
  expect('/api','cors','empty','same-origin');
  expect('/post','cors','empty','same-origin');
  expect('/s.css','no-cors','style','same-origin');
  expect('/app.js','no-cors','script','same-origin');
  expect('/p.gif','no-cors','image','same-origin');
  // A srcdoc frame has the origin of its parent: its image is same-origin, not "cross-site".
  expect('/p2.gif','no-cors','image','same-origin');
  expect('other/x.gif','no-cors','image','cross-site');
  assert.equal((await c.send('close')).ok,true);
});

// #2724: a cross-origin frame is a navigation the driver refuses (the host allow-list), so it never
// reaches the other server and the session reports driver.host_refused; allowing its origin for
// subresources does not turn it into an allowed frame.
test('a cross-origin iframe navigation is refused and never reaches the other server',async t=>{
  const hits=[];
  const other=createServer((req,res)=>{hits.push({path:req.url,site:req.headers['sec-fetch-site']??'-'});res.end('<p>other</p>');});
  const main=createServer((req,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end(`<h1>Page</h1><iframe src="http://other.localhost:${other.address().port}/xframe" title="cross"></iframe>`);});
  await Promise.all([other,main].map(server=>new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))));
  t.after(()=>Promise.all([other,main].map(server=>new Promise(resolve=>server.close(resolve)))));
  const c=await client(t);
  const r=await c.send('open',{base:`http://127.0.0.1:${main.address().port}/`,viewport,allowOrigins:[`http://other.localhost:${other.address().port}`]});
  await new Promise(resolve=>setTimeout(resolve,500));
  const snap=r.ok ? await c.send('snapshot',{expect:[]}) : r;
  assert.equal(snap.code,'driver.host_refused',JSON.stringify(snap));
  assert.deepEqual(hits,[],'the refused frame must not reach the other server');
});

// #2837: a `tool` act calls a WebMCP page tool. The tool is the hands; the screen is the proof.
// Gap: the other cells act by role and name; none proves a tool the page registered runs for real,
// nor that its returned text never decides the step. Cost: one Chromium session per cell (~3 s;
// ~25 s for the one that waits out an unmet expectation), local fixture only.
const toolLocator=name=>({role:'webmcp',name,exact:true,testId:null,context:null,nth:null});

test('a tool act runs the page tool and the reached screen is judged by what is rendered',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);await open(c,f.base+'/webmcp',[],{webmcp:true});
  const r=await act(c,'tool','webmcp','rename_heading',{arguments:{title:'Renamed by a tool'}});
  assert.equal(r.ok,true,JSON.stringify(r));
  assert.equal(r.result.toolResult,'ok');
  assert.deepEqual(r.result.locator,toolLocator('rename_heading'));
  const seen=await c.send('snapshot',{expect:[{role:'heading',name:'Renamed by a tool'}]});
  assert.equal(seen.ok,true,JSON.stringify(seen));
  // No arguments means `{}`: the tool still runs and its default applies.
  const bare=await act(c,'tool','webmcp','rename_heading');assert.equal(bare.ok,true,JSON.stringify(bare));
  assert.equal((await c.send('snapshot',{expect:[{role:'heading',name:'Renamed'}]})).ok,true);
});

test('the tool result never judges the step: a tool that answers ok but changes nothing fails the screen',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const c=await client(t);await open(c,f.base+'/webmcp',[],{webmcp:true});
  const r=await act(c,'tool','webmcp','says_ok');
  assert.equal(r.ok,true,JSON.stringify(r));assert.equal(r.result.toolResult,'ok');
  const seen=await c.send('snapshot',{expect:[{role:'heading',name:'Renamed'}]});
  assert.equal(seen.ok,false);assert.equal(seen.code,'driver.expectation_failed');
});

test('a tool act names its failure: missing, ambiguous, thrown, or no WebMCP in the browser',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const missing=await client(t);await open(missing,f.base+'/webmcp',[],{webmcp:true});
  assert.equal((await act(missing,'tool','webmcp','no_such_tool')).code,'driver.tool_missing');
  // The browser lists a same-origin frame's tools with the page's: two tools named `twin`.
  const twin=await client(t);await open(twin,f.base+'/webmcp-twin',[],{webmcp:true});
  // The frame's tool is registered once the frame has loaded; the page says so in its heading.
  assert.equal((await twin.send('snapshot',{expect:[{role:'heading',name:'Twins ready'}]})).ok,true);
  assert.equal((await act(twin,'tool','webmcp','twin')).code,'driver.tool_ambiguous');
  const thrown=await client(t);await open(thrown,f.base+'/webmcp',[],{webmcp:true});
  assert.equal((await act(thrown,'tool','webmcp','explode')).code,'driver.tool_failed');
  // Opened without `webmcp` the browser has no `document.modelContext`: the observer cannot do
  // this act, which is not a missing tool and never a pass.
  const without=await client(t);await open(without,f.base+'/webmcp');
  assert.equal((await act(without,'tool','webmcp','rename_heading')).code,'driver.webmcp_unavailable');
});

test('a tool result is recorded masked and cut, and a secret never reaches the reply',async t=>{
  const secret='CANARY_tool_result_2837';
  const server=createServer((_,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end(`<h1>Tool</h1><script>const mc=document.modelContext;mc.registerTool({name:'echo',description:'echo',inputSchema:{type:'object'},execute:async()=>'token ${secret} end'});mc.registerTool({name:'long_result',description:'long',inputSchema:{type:'object'},execute:async()=>'x'.repeat(4000)});mc.registerTool({name:'rename_heading',description:'rename',inputSchema:{type:'object'},execute:async()=>'ok'});</script>`);});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  t.after(()=>new Promise(resolve=>server.close(resolve)));
  const c=await client(t,{GRAPHHELM_SECRET_TOOL:secret});
  await open(c,`http://127.0.0.1:${server.address().port}/`,[],{webmcp:true,survive:true});
  const r=await act(c,'tool','webmcp','echo');
  assert.equal(r.ok,true,JSON.stringify(r));
  assert.equal(r.result.toolResult,'token «secret:TOOL» end');
  assert.ok(!JSON.stringify(r).includes(secret));
  const long=await act(c,'tool','webmcp','long_result');
  assert.equal(long.ok,true,JSON.stringify(long));
  assert.ok(Buffer.byteLength(long.result.toolResult)<=512,String(long.result.toolResult.length));
  // A secret named in the act's own arguments is refused before the page sees it.
  const leaked=await act(c,'tool','webmcp','rename_heading',{arguments:{title:secret}});
  assert.equal(leaked.code,'driver.secret_literal');
});

// #2837: with a session open, a malformed tool act is refused before the page sees it. Without a
// session every one of these would read protocol_invalid for the wrong reason, so each is sent to
// a live one.
test('a tool act needs role webmcp, no text or secret, and arguments that are a bounded JSON object',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  let deep='leaf';for(let i=0;i<9;i++)deep={k:deep};
  for(const extra of [{role:'button'},{secretEnv:'GRAPHHELM_SECRET_X'},{text:'{}'},{arguments:[1]},{arguments:'x'},{arguments:null},{arguments:{big:'x'.repeat(5000)}},{arguments:deep}]) {
    const c=await client(t,{GRAPHHELM_SECRET_X:'CANARY_tool_secret_2837'});await open(c,f.base+'/webmcp',[],{webmcp:true});
    const r=await c.send('act',{kind:'tool',role:'webmcp',name:'says_ok',...extra});
    assert.equal(r.code,'driver.protocol_invalid',JSON.stringify(extra).slice(0,80));
  }
  // `arguments` belongs to the tool kind alone.
  const c=await client(t);await open(c,f.base+'/webmcp',[],{webmcp:true});
  assert.equal((await c.send('act',{kind:'activate',role:'button',name:'Save',arguments:{}})).code,'driver.protocol_invalid');
});

// #2848: a capture paints only secrets and outlines every other editable field.
// Contract: what a plain field holds stays readable in the frame, a secret never does.
// Gap: no earlier cell decoded a plain field's pixels; they were all painted. Cost: ~10 seconds.
const MAGENTA=[255,0,255,255];
async function pixelOracle(t,base) {
  const {chromium}=createRequire(join(project,'package.json'))('@playwright/test');
  const browser=await chromium.launch({headless:true});t.after(()=>browser.close());
  const page=await browser.newPage({viewport});await page.goto(base);
  const box=async name=>{const b=await page.getByLabel(name,{exact:true}).boundingBox();assert.ok(b,name);return b;};
  const decode=async bytes=>{
    const shot=await browser.newPage({viewport});
    await shot.setContent('<canvas id="c"></canvas>');
    await shot.evaluate(async data=>{const image=new Image();image.src='data:image/png;base64,'+data;await image.decode();const c=document.getElementById('c');c.width=image.width;c.height=image.height;window.ctx=c.getContext('2d');window.ctx.drawImage(image,0,0);},bytes.toString('base64'));
    return {
      /** Inside the box (inset 4px, clear of its border and outline): dark ink and magenta pixel counts and the total. */
      inside:b=>shot.evaluate(({x,y,width,height})=>{const d=window.ctx.getImageData(Math.round(x)+4,Math.round(y)+4,Math.round(width)-8,Math.round(height)-8).data;let ink=0,magenta=0;for(let i=0;i<d.length;i+=4){if(d[i]<100&&d[i+1]<100&&d[i+2]<100)ink++;if(d[i]===255&&d[i+1]===0&&d[i+2]===255&&d[i+3]===255)magenta++;}return {ink,magenta,total:d.length/4};},b),
      /** The pixel 2px outside the box's left, right, top and bottom edge midpoints (the outline is 1px off, 2px wide). */
      ring:b=>shot.evaluate(({x,y,width,height})=>{const at=(px,py)=>Array.from(window.ctx.getImageData(Math.round(px),Math.round(py),1,1).data);const mx=x+width/2,my=y+height/2;return [at(x-2,my),at(x+width+1,my),at(mx,y-2),at(mx,y+height+1)];},b),
    };
  };
  return {box,decode};
}
async function fingerprint(c) {
  assert.equal((await act(c,'activate','button','Fingerprint')).ok,true);
  const s=await c.send('snapshot',{expect:[]});assert.equal(s.ok,true);
  const m=/fp (\d+) (\d+)/.exec(s.result.ariaYaml);assert.ok(m,s.result.ariaYaml);return m[0];
}

test('a capture shows what a plain field holds with a magenta outline and paints only secrets',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const secret='CANARY_fields_secret_2848';
  const oracle=await pixelOracle(t,f.base+'/fields');
  const c=await client(t,{GRAPHHELM_SECRET_KEY:secret});await open(c,f.base+'/fields');
  for(const [role,name,text] of [['textbox','Email','owner@example.test'],['searchbox','Find','review'],['textbox','Note','Use the slot'],['textbox','Notes box','Edited note'],['textbox','Password','plain-pass-1']]) assert.equal((await act(c,'enter_text',role,name,{text})).ok,true,name);
  assert.equal((await act(c,'enter_text','textbox','API key',{secretEnv:'GRAPHHELM_SECRET_KEY'})).ok,true);
  const before=await fingerprint(c);
  const boxes={};for(const name of ['Email','Find','Note','Notes box','Password','API key'])boxes[name]=await oracle.box(name);
  const capture=await c.send('capture',{path:'fields.png',maskSecrets:true});assert.equal(capture.ok,true,JSON.stringify(capture));assert.equal(capture.result.masked,true);
  const image=await oracle.decode(await readFile(join(c.output,'fields.png')));
  // (a) plain fields: content visible, outline around the box.
  for(const name of ['Email','Find','Note','Notes box']) {
    const inside=await image.inside(boxes[name]);
    assert.ok(inside.ink>20,`${name} content is readable`);assert.equal(inside.magenta,0,`${name} is not painted`);
    for(const pixel of await image.ring(boxes[name])) assert.deepEqual(pixel,MAGENTA,`${name} has the outline`);
  }
  // (b), (c) a password field and a field filled from a secret are painted whole.
  for(const name of ['Password','API key']) {const inside=await image.inside(boxes[name]);assert.equal(inside.magenta,inside.total,`${name} is painted`);}
  // (c) the visible echo of the secret is painted too.
  const echoBox={x:40,y:boxes['API key'].y+boxes['API key'].height+6,width:300,height:30};
  const echoInside=await image.inside(echoBox);assert.equal(echoInside.magenta,echoInside.total,'the echo is painted');
  // (f) the page is as it was: DOM, inline styles, stylesheets and every field's box.
  assert.equal(await fingerprint(c),before);
  // (g) the secret field is still painted after the page rebuilt it under another name.
  assert.equal((await act(c,'activate','button','Re-render')).ok,true);
  assert.equal((await c.send('capture',{path:'rebuilt.png',maskSecrets:true})).ok,true);
  const rebuilt=await oracle.decode(await readFile(join(c.output,'rebuilt.png')));
  const inside=await rebuilt.inside(boxes['API key']);assert.equal(inside.magenta,inside.total,'the rebuilt secret field is painted');
  assert.equal((await c.send('close')).ok,true);
});

test('a credential autocomplete paints a text field and an ordinary one does not',async t=>{
  const f=await startFixture();t.after(()=>f.close());
  const oracle=await pixelOracle(t,f.base+'/autocomplete');
  const c=await client(t);await open(c,f.base+'/autocomplete');
  const names=['current-password','new-password','one-time-code','cc-number','cc-csc','cc-exp','cc-exp-month','cc-exp-year','email'];
  for(const v of names) assert.equal((await act(c,'enter_text','textbox','Auto '+v,{text:'abcd1234'})).ok,true,v);
  assert.equal((await c.send('capture',{path:'auto.png',maskSecrets:true})).ok,true);
  const image=await oracle.decode(await readFile(join(c.output,'auto.png')));
  for(const v of names) {
    const inside=await image.inside(await oracle.box('Auto '+v));
    if(v==='email'){assert.equal(inside.magenta,0,'an ordinary autocomplete is not painted');assert.ok(inside.ink>20);}
    else assert.equal(inside.magenta,inside.total,`${v} is painted`);
  }
  assert.equal((await c.send('close')).ok,true);
});

// #2848 review: a known secret drawn inside an editable field is painted however the field shows it
// (placeholder, other case, spaced, truncated) and the paint never depends on the DOM staying still.
const ECHO_SECRET='sk-ECHO-Probe-2848-zz';
const ECHO_CSS='body{margin:0;background:#fff;color:#000;font:16px Arial}h1{margin:8px 40px;font-size:20px}.f{display:block;margin:6px 40px;width:300px;height:30px;box-sizing:border-box;border:1px solid #888;background:#fff;color:#000;font:16px Arial}div.f{padding:4px}';
async function echoServer(t,body) {
  const server=createServer((req,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end(`<!doctype html><html><head><meta charset="utf-8"><title>Echo</title><style>${ECHO_CSS}</style></head><body><h1>Echo</h1>${body(new URL(req.url,'http://x').searchParams)}</body></html>`);});
  await new Promise(done=>server.listen(0,'127.0.0.1',done));t.after(()=>new Promise(done=>server.close(done)));
  return `http://127.0.0.1:${server.address().port}`;
}
const spaced=ECHO_SECRET.replace(/(...)/g,'$1 ');
const echoFields=[
  ['Placeholder echo',`<input class="f" aria-label="Placeholder echo" placeholder="${ECHO_SECRET}">`,true],
  ['Placeholder over value',`<input class="f" aria-label="Placeholder over value" placeholder="${ECHO_SECRET}" value="note text">`,false],
  ['Upper case',`<input class="f" aria-label="Upper case" value="${ECHO_SECRET.toUpperCase()}">`,true],
  ['Spaced',`<input class="f" aria-label="Spaced" value="${spaced}">`,true],
  ['Truncated',`<input class="f" aria-label="Truncated" value="${ECHO_SECRET.slice(0,8)}…">`,true],
  // A short value that is not a piece of the secret stays readable; one that is a piece is painted.
  ['Coincidence',`<input class="f" aria-label="Coincidence" value="xqz">`,false],
  ['Short piece',`<input class="f" aria-label="Short piece" value="${ECHO_SECRET.slice(0,3)}">`,true],
  // Accepted over-painting: a plain value that shares 4 letters or digits with the secret is painted.
  ['Four-character coincidence',`<input class="f" aria-label="Four-character coincidence" value="be 2848">`,true],
  ['Editable echo',`<div class="f" role="textbox" aria-label="Editable echo" contenteditable="true">${ECHO_SECRET}</div>`,true],
  ['Plain',`<input class="f" aria-label="Plain" value="plain note">`,false],
];
test('a secret shown in an editable field is painted whatever its placeholder, case, spacing or truncation',async t=>{
  const base=await echoServer(t,()=>echoFields.map(([,html])=>html).join(''));
  const oracle=await pixelOracle(t,base);
  const c=await client(t,{GRAPHHELM_SECRET_KEY:ECHO_SECRET});await open(c,base);
  const capture=await c.send('capture',{path:'echo.png',maskSecrets:true});assert.equal(capture.ok,true,JSON.stringify(capture));
  const image=await oracle.decode(await readFile(join(c.output,'echo.png')));
  for(const [name,,painted] of echoFields) {
    const inside=await image.inside(await oracle.box(name));
    if(painted) assert.equal(inside.magenta,inside.total,`${name} is painted`);
    else {assert.equal(inside.magenta,0,`${name} is not painted`);assert.ok(inside.ink>20,`${name} stays readable`);}
  }
  assert.equal((await c.send('close')).ok,true);
});

test('a field whose value cannot be read is painted, not skipped',async t=>{
  const base=await echoServer(t,()=>`<input class="f" aria-label="Throwing" value="x"><script>Object.defineProperty(document.querySelector('[aria-label=Throwing]'),'value',{get(){throw new Error('no')}})</script>`);
  const oracle=await pixelOracle(t,base);
  const c=await client(t,{GRAPHHELM_SECRET_KEY:ECHO_SECRET});await open(c,base);
  const capture=await c.send('capture',{path:'throws.png',maskSecrets:true});assert.equal(capture.ok,true,JSON.stringify(capture));
  const inside=await (await oracle.decode(await readFile(join(c.output,'throws.png')))).inside(await oracle.box('Throwing'));
  assert.equal(inside.magenta,inside.total,'the unreadable field is painted');
});

// A page that adds a field between the driver's read and its screenshot must not leave the secret unpainted:
// one retry on the new DOM, then a refusal that keeps no image.
const churnBody=params=>`<input class="f" aria-label="Holds secret" value="${ECHO_SECRET}"><input class="f" aria-label="Other" value="other note">${params.has('quiet')?'':`<script>let done=false;const mode=${JSON.stringify(params.get('mode'))};new MutationObserver(list=>{if(!list.some(m=>m.target.style&&m.target.style.outline))return;if(mode==='always'||!done){done=true;const n=document.createElement('input');n.className='f';n.setAttribute('aria-label','Inserted');n.style.cssText='position:absolute;left:600px;top:10px;width:100px';document.body.prepend(n)}}).observe(document.body,{attributes:true,subtree:true,attributeFilter:['style']})</script>`}`;
test('a field added between the read and the screenshot never leaves a secret unpainted',async t=>{
  const base=await echoServer(t,churnBody);
  const oracle=await pixelOracle(t,base+'/?quiet=1');
  const c=await client(t,{GRAPHHELM_SECRET_KEY:ECHO_SECRET});await open(c,base+'/?mode=once');
  const capture=await c.send('capture',{path:'once.png',maskSecrets:true});
  assert.equal(capture.ok,true,JSON.stringify(capture));
  const image=await oracle.decode(await readFile(join(c.output,'once.png')));
  const inside=await image.inside(await oracle.box('Holds secret'));assert.equal(inside.magenta,inside.total,'the secret field is painted after one retry');
  const always=await client(t,{GRAPHHELM_SECRET_KEY:ECHO_SECRET});await open(always,base+'/?mode=always');
  assert.equal((await always.send('capture',{path:'always.png',maskSecrets:true})).code,'driver.capture_refused');
  await assert.rejects(readFile(join(always.output,'always.png')),{code:'ENOENT'});
});

// #2848 second review: a declared secret reformatted inside another editable field (separators, a leading
// ellipsis, a label plus a cut) is painted; an unrelated plain field stays readable and outlined.
const CARD_SECRET='4242424242424242';
const reformatted=[
  ['Dashed copy','4242-4242-4242-4242',true],
  ['Separated every character','a-1-b-2-c-3-d-4-e-5',true],
  ['Leading ellipsis','…'+ECHO_SECRET.slice(-10),true],
  ['Underscored','sk_ECHO_Probe_2848_zz',true],
  ['Label and cut','key: '+ECHO_SECRET.slice(0,12)+'…',true],
  ['Unrelated','plain note',false],
];
test('a secret reformatted inside an editable field is painted and an unrelated field is not',async t=>{
  const base=await echoServer(t,()=>reformatted.map(([name,value])=>`<input class="f" aria-label="${name}" value="${value}">`).join(''));
  const oracle=await pixelOracle(t,base);
  const c=await client(t,{GRAPHHELM_SECRET_KEY:ECHO_SECRET,GRAPHHELM_SECRET_CARD:CARD_SECRET,GRAPHHELM_SECRET_CODE:'a1b2c3d4e5'});await open(c,base);
  const capture=await c.send('capture',{path:'reformatted.png',maskSecrets:true});assert.equal(capture.ok,true,JSON.stringify(capture));
  const image=await oracle.decode(await readFile(join(c.output,'reformatted.png')));
  const unpainted=[];
  for(const [name,,painted] of reformatted) {
    const inside=await image.inside(await oracle.box(name));
    if(painted) {if(inside.magenta!==inside.total)unpainted.push(name);}
    else {assert.equal(inside.magenta,0,`${name} is not painted`);assert.ok(inside.ink>20,`${name} stays readable`);for(const pixel of await image.ring(await oracle.box(name))) assert.deepEqual(pixel,MAGENTA,`${name} keeps its outline`);}
  }
  assert.deepEqual(unpainted,[],'every reformatted copy of a secret is painted');
});

// A page that writes the secret into an existing plain field after the read and before the screenshot:
// the paint decision is read again after the screenshot, so the capture is repeated and the field painted.
test('a secret written into an existing field between the read and the screenshot is painted after a retry',async t=>{
  const base=await echoServer(t,()=>`<input class="f" aria-label="Late" value="plain note"><script>let done=false;new MutationObserver(list=>{if(done||!list.some(m=>m.target.style&&m.target.style.outline))return;done=true;document.querySelector('[aria-label=Late]').value=${JSON.stringify(ECHO_SECRET)}}).observe(document.body,{attributes:true,subtree:true,attributeFilter:['style']})</script>`);
  const oracle=await pixelOracle(t,base);
  const c=await client(t,{GRAPHHELM_SECRET_KEY:ECHO_SECRET});await open(c,base);
  const capture=await c.send('capture',{path:'late.png',maskSecrets:true});
  if(capture.ok) {
    const inside=await (await oracle.decode(await readFile(join(c.output,'late.png')))).inside(await oracle.box('Late'));
    assert.equal(inside.magenta,inside.total,'the late secret is painted, never kept readable');
  } else assert.equal(capture.code,'driver.capture_refused');
});

// #2848 third review: a secret split across several editable fields (a one-time-code widget, pairs,
// 3-character pieces) is painted piece by piece: a field whose whole drawn text is a piece of a secret is painted.
const OTP_SECRET='482915';
async function paintedBoxes(t,base,names,env,prepare=async()=>{}) {
  const oracle=await pixelOracle(t,base);
  const c=await client(t,env);await open(c,base);await prepare(c);
  const capture=await c.send('capture',{path:'pieces.png',maskSecrets:true});assert.equal(capture.ok,true,JSON.stringify(capture));
  const image=await oracle.decode(await readFile(join(c.output,'pieces.png')));
  const unpainted=[];
  for(const name of names){const inside=await image.inside(await oracle.box(name));if(inside.magenta!==inside.total)unpainted.push(name);}
  return unpainted;
}
test('six one-digit boxes spread from a filled first box are all painted',async t=>{
  const names=[1,2,3,4,5,6].map(n=>'Digit '+n);
  const base=await echoServer(t,()=>names.map((name,index)=>`<input class="f" aria-label="${name}" maxlength="6" ${index===0?'autocomplete="one-time-code" oninput="const v=this.value;const all=[...document.querySelectorAll(\'input\')];all.forEach((box,i)=>{box.value=v[i]??\'\'})"':''}>`).join(''));
  const unpainted=await paintedBoxes(t,base,names,{GRAPHHELM_SECRET_CODE:OTP_SECRET},async c=>assert.equal((await act(c,'enter_text','textbox','Digit 1',{secretEnv:'GRAPHHELM_SECRET_CODE'})).ok,true));
  assert.deepEqual(unpainted,[],'every box of the code is painted');
});
test('a secret shown in pairs or in 3-character pieces is painted piece by piece',async t=>{
  const pairs=[['Pair 1','48'],['Pair 2','29'],['Pair 3','15']];
  const pieces=[['Piece 1','ske'],['Piece 2','cho'],['Piece 3','pro'],['Piece 4','be2'],['Piece 5','848'],['Piece 6','zz']];
  const rows=[...pairs,...pieces,['Unrelated piece','xqz']];
  const base=await echoServer(t,()=>rows.map(([name,value])=>`<input class="f" aria-label="${name}" value="${value}">`).join(''));
  const unpainted=await paintedBoxes(t,base,[...pairs,...pieces].map(([name])=>name),{GRAPHHELM_SECRET_CODE:OTP_SECRET,GRAPHHELM_SECRET_KEY:ECHO_SECRET});
  assert.deepEqual(unpainted,[],'every piece of a secret is painted');
});
