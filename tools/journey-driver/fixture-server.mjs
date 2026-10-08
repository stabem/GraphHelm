import { createServer } from 'node:http';
import { pathToFileURL } from 'node:url';
import {readFileSync,writeFileSync} from 'node:fs';

// Observer-owned static app. No production accounts, provider, or credential storage.
export async function startFixture() {
  let canaryCount = 0, modelCount = 0, deleteCount = 0, fault=null;
  const canary = createServer((req,res) => {
    canaryCount++;
    res.setHeader('Access-Control-Allow-Origin','*');
    res.end(req.url === '/image' ? '' : 'canary');
  });
  canary.on('upgrade',(_req,socket) => {canaryCount++;socket.destroy();});
  await new Promise(done=>canary.listen(0,'127.0.0.1',done));
  const canaryOrigin=`http://127.0.0.1:${canary.address().port}`;
  const model = createServer((_req,res) => {modelCount++;res.writeHead(503);res.end('model calls forbidden');});
  await new Promise(done=>model.listen(0,'127.0.0.1',done));
  const shell = body => `<!doctype html><html><head><meta charset="utf-8"><title>Replay fixture</title><style>body{font-family:Arial}input{position:absolute;left:40px;top:80px;width:240px;height:36px}#echo{position:absolute;left:40px;top:140px}</style></head><body>${body}</body></html>`;
  const app=createServer((req,res) => {
    const u=new URL(req.url,'http://fixture');
    res.setHeader('Content-Type','text/html; charset=utf-8');
    if (u.pathname==='/checkout' && fault?.kind==='edit-flow') {
      const file=fault.path;fault=null;writeFileSync(file,readFileSync(file,'utf8')+'# changed during replay\n');
    }
    if (u.pathname==='/checkout' && fault?.kind==='missing-checkout') {
      res.end(shell('<main><h1>Checkout</h1></main>'));return;
    }
    // #398: the cart's Checkout control renamed (no test id), so its cached locator drifts.
    if (u.pathname==='/cart' && fault?.kind==='rename-checkout') {
      res.end(shell(`<main><h1>Cart</h1><button onclick="location.href='/checkout'">Proceed</button><a href="/guest">Guest checkout</a></main>`));return;
    }
    // #356 drift variants: a wrong destination and a changed destination.
    if (u.pathname==='/cart' && fault?.kind==='wrong-url') {
      res.end(shell('<main><h1>Cart</h1><button data-testid="checkout" onclick="location.href=\'/guest\'">Checkout</button><a href="/guest">Guest checkout</a></main>'));return;
    }
    if (u.pathname==='/checkout' && fault?.kind==='changed-checkout') {
      res.end(shell('<main><h1>Checkout</h1><label>Password<input aria-label="Password" type="password"></label><button>Apply coupon</button><button>Gift wrap</button><button>Split payment</button><a href="/help">Help</a><a href="/terms">Terms</a><button>Save cart</button></main>'));return;
    }
    if(u.pathname==='/account/delete' && req.method==='POST') {deleteCount++;res.end(shell('<main><h1>Account deleted</h1></main>'));return;}
    if(u.pathname==='/account') res.end(shell('<main><h1>Account</h1><form method="post" action="/account/delete"><button>Delete account</button></form></main>'));
    else if(u.pathname==='/cart') res.end(shell('<main><h1>Cart</h1><button data-testid="checkout" onclick="location.href=\'/checkout\'">Checkout</button><a href="/guest">Guest checkout</a></main>'));
    else if(u.pathname==='/checkout') res.end(shell('<main><h1>Checkout</h1><label>Password<input aria-label="Password" type="password" oninput="document.getElementById(\'echo\').textContent=this.value"></label><div id="echo"></div><button style="display:block;margin-top:160px" onclick="location.href=\'/orders/42\'">Submit order</button></main>'));
    else if(u.pathname==='/guest') res.end(shell('<main><h1>Guest checkout</h1><button onclick="location.href=\'/orders/42\'">Place guest order</button></main>'));
    else if(u.pathname==='/orders/42') res.end(shell('<main><h1>Order 42</h1><button>Continue</button></main>'));
    else if(u.pathname==='/big') res.end(shell('<main><h1>Big page</h1>'+Array.from({length:120},(_,i)=>`<section aria-label="Part ${i}"><h2>Section ${i}</h2><p>Long paragraph text that only a reader needs, repeated so the page passes the model budget.</p><button>Open ${i}</button></section>`).join('')+'</main>'));
    else if(u.pathname==='/controls') res.end(shell(`<main><h1>Controls</h1><button onclick="document.querySelector('h1').textContent='Saved'">Save</button><button>Save as draft</button><button style="display:none">Hidden</button><section role="region" aria-label="Billing"><button data-testid="billing-save">Save duplicate</button></section><section role="region" aria-label="Shipping"><button>Save duplicate</button></section><button data-testid="wrong-role">Other</button><button onclick="setTimeout(()=>document.getElementById('delayed').hidden=false,150)">Reveal delayed</button><button id="delayed" hidden>Delayed</button><label>Password<input aria-label="Password" type="password" oninput="document.getElementById('echo').textContent=this.value"></label><div id="echo"></div></main>`));
    else if(u.pathname==='/network') res.end(shell(`<main><h1>Network</h1><button onclick="fetch('${canaryOrigin}/fetch').catch(()=>{})">Fetch</button><button onclick="window.open('${canaryOrigin}/popup')">Popup</button><button onclick="let f=document.createElement('iframe');f.src='${canaryOrigin}/frame';document.body.append(f)">Frame</button><button onclick="new WebSocket('${canaryOrigin.replace('http:','ws:')}/ws')">WebSocket</button><button onclick="navigator.serviceWorker.register('/sw.js').catch(()=>{})">Service worker</button><button onclick="location.href='/redirect'">Redirect</button><button onclick="location.href='/redirect-local'">Redirect chain</button></main>`));
    else if(u.pathname==='/redirect-local') {res.writeHead(302,{Location:'/redirect'});res.end();}
    else if(u.pathname==='/redirect') {res.writeHead(302,{Location:canaryOrigin+'/redirected'});res.end();}
    else if(u.pathname==='/sw.js') {res.setHeader('Content-Type','application/javascript');res.end(`fetch('${canaryOrigin}/worker');`);}
    else if(u.pathname==='/late') res.end(shell(`<main><h1 id="title">Loading</h1><script>setTimeout(()=>{document.getElementById('title').textContent='Ready';const b=document.createElement('button');b.textContent='Continue';document.querySelector('main').append(b);},1500)</script></main>`));
    else {res.writeHead(404);res.end(shell('<h1>Missing</h1>'));}
  });
  await new Promise(done=>app.listen(0,'127.0.0.1',done));
  return {
    base:`http://127.0.0.1:${app.address().port}`,canaryOrigin,
    modelOrigin:`http://127.0.0.1:${model.address().port}`,
    arm:value=>{fault=value;},
    counts:()=>({canary:canaryCount,model:modelCount,deletes:deleteCount}),
    reset:()=>{canaryCount=0;modelCount=0;deleteCount=0;},
    close:async()=>{for(const server of [app,canary,model]) {server.closeAllConnections();await new Promise(done=>server.close(done));}},
  };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const fixture=await startFixture();
  process.stdout.write(JSON.stringify({base:fixture.base,canaryOrigin:fixture.canaryOrigin,modelOrigin:fixture.modelOrigin})+'\n');
  process.stdin.resume();
  process.stdin.on('data',chunk=>{
    for(const command of chunk.toString().trim().split('\n')) {
      if(command==='counts') process.stdout.write(JSON.stringify(fixture.counts())+'\n');
      else if(command==='reset') {fixture.reset();fixture.arm(null);process.stdout.write('{"reset":true}\n');}
      else if(command.startsWith('{')) {fixture.arm(JSON.parse(command));process.stdout.write('{"armed":true}\n');}
    }
  });
  const close=async()=>{await fixture.close();process.exit(0);};
  process.stdin.on('end',close);process.on('SIGTERM',close);process.on('SIGINT',close);
}
