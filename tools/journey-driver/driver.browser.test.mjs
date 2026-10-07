import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
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
  const child=spawn(process.execPath,[driver,'--project',project,'--output-dir',output],{env:{PATH:process.env.PATH,SystemRoot:process.env.SystemRoot,TEMP:process.env.TEMP,TMP:process.env.TMP,USERPROFILE:process.env.USERPROFILE,HOME:process.env.HOME,LOCALAPPDATA:process.env.LOCALAPPDATA,...env},stdio:['pipe','pipe','pipe']});
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
async function open(c,base,allowOrigins=[]) {const r=await c.send('open',{base,viewport,allowOrigins});assert.equal(r.ok,true,JSON.stringify(r));}
async function act(c,kind,role,name,extra={}) {return c.send('act',{kind,role,name,...extra});}

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
