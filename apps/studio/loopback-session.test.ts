import { createServer, type ViteDevServer } from "vite";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer as httpServer, request, type Server } from "node:http";
import { afterAll, beforeAll, expect, it } from "vitest";
import { devSession } from "./dev-session";
let server: ViteDevServer;
let root: string;
let port: number;
let upstream: Server;
let upstreamHeaders: Record<string,unknown> = {};
function call(path: string, headers: Record<string,string> = {}, method = "GET", body?: string): Promise<{status:number; headers: Record<string,unknown>; body:string}> {
  return new Promise((resolve,reject) => {
    const req = request({host:"127.0.0.1",port,path,method,headers}, res => {
      let text=""; res.on("data",chunk=>text+=chunk); res.on("end",()=>resolve({status:res.statusCode!,headers:res.headers,body:text}));
    }); req.on("error",reject); req.end(body);
  });
}
beforeAll(async()=>{
  root=mkdtempSync(join(tmpdir(),"studio-cookie-")); mkdirSync(join(root,".graphhelm"));
  writeFileSync(join(root,".graphhelm/events.token"),"fixture-owner-secret");
  upstream=httpServer((req,res)=>{upstreamHeaders=req.headers;res.setHeader("content-type","application/json");res.end('{"ok":true}');});
  await new Promise<void>(resolve=>upstream.listen(0,"127.0.0.1",resolve));
  writeFileSync(join(root,".graphhelm/events.runtime-lifecycle.json"),JSON.stringify({state:"running",address:`127.0.0.1:${(upstream.address() as {port:number}).port}`}));
  writeFileSync(join(root,"index.html"),"<h1>Demo Studio</h1>");
  server=await createServer({configFile:false,root,plugins:[devSession({GRAPHHELM_EVENTS:join(root,".graphhelm/events")})],server:{host:"127.0.0.1",port:0},logLevel:"silent"});
  await server.listen(); port=(server.httpServer!.address() as {port:number}).port;
});
afterAll(async()=>{await server?.close(); await new Promise<void>(resolve=>upstream?.close(()=>resolve())); if(root)rmSync(root,{recursive:true,force:true});});
it("bootstraps an HttpOnly session from a ticket-free deep link without returning Runtime credentials",async()=>{
  const shell=await call("/?view=proof&journey=demo&run=demo-run",{"sec-fetch-mode":"navigate","sec-fetch-dest":"document","sec-fetch-site":"none"});
  expect(shell.status).toBe(200);
  const cookies=shell.headers["set-cookie"] as string[];
  expect(cookies?.[0]).toContain("HttpOnly"); expect(cookies?.[0]).toContain("SameSite=Strict");
  const session=await call("/__studio/session",{cookie:cookies[0]!.split(";")[0]!,"sec-fetch-site":"same-origin"});
  expect(session.status).toBe(200); expect(JSON.parse(session.body).ok).toBe(true);
  expect(session.body).not.toMatch(/fixture-owner-secret|sessionKey|"token"/);
});
it.each(["https://evil.example","null","http://localhost:1"])("rejects Origin %s before serving the shell",async origin=>{
  expect((await call("/",{origin})).status).toBe(403);
});
it("rejects foreign Host even with an Origin that repeats it",async()=>{
  expect((await call("/",{host:`evil.example:${port}`,origin:`http://evil.example:${port}`})).status).toBe(403);
});
it("refuses cookieless protected routes and cross-site shell loads",async()=>{
  for(const path of ["/__studio/session","/__studio/identity","/v1/projects","/health","/p/"+"a".repeat(64)+"/health"])
    expect((await call(path,{"sec-fetch-site":"same-origin"})).status).toBe(403);
  expect((await call("/",{"sec-fetch-site":"cross-site"})).status).toBe(403);
});
it("never serves a project credential from Vite's static root",async()=>{
  for (const path of ["/.graphhelm/events.token", `/@fs/${join(root,".graphhelm/events.token").replaceAll("\\","/")}`]) {
    const result = await call(path);
    expect(result.status).not.toBe(200);
    expect(result.body).not.toContain("fixture-owner-secret");
  }
});

async function browserHeaders() {
 const shell=await call("/",{"sec-fetch-mode":"navigate","sec-fetch-dest":"document","sec-fetch-site":"none"});
 return {cookie:(shell.headers["set-cookie"] as string[])[0]!.split(";")[0]!,"sec-fetch-site":"same-origin"};
}
it("requires cookie and origin for mutation; originless reads require same-origin metadata",async()=>{
 const headers=await browserHeaders();
 expect((await call("/__studio/session",{cookie:headers.cookie})).status).toBe(403);
 expect((await call("/__studio/add",headers,"POST",JSON.stringify({path:root}))).status).toBe(403);
 expect((await call("/__studio/add",{...headers,origin:`http://127.0.0.1:${port}`} ,"POST",JSON.stringify({path:root}))).status).toBe(200);
 expect((await call("/__studio/session?project="+"b".repeat(64),headers)).status).toBe(404);
 expect((await call("/p/"+"b".repeat(64)+"/health",headers)).status).toBe(404);
});
it("injects the selected Runtime credential without forwarding browser cookies or authorization",async()=>{
 const headers=await browserHeaders();
 expect((await call("/health",{...headers,authorization:"Bearer attacker"})).status).toBe(200);
 expect(upstreamHeaders.authorization).toBe("Bearer fixture-owner-secret"); expect(upstreamHeaders.cookie).toBeUndefined();
});
it("registers a local CLI project with its own credential without issuing a browser cookie",async()=>{
 const body=JSON.stringify({path:root});
 expect((await call("/__studio/register",{authorization:"Bearer wrong"},"POST",body)).status).toBe(403);
 const registered=await call("/__studio/register",{authorization:"Bearer fixture-owner-secret"},"POST",body);
 expect(registered.status).toBe(200); expect(registered.headers["set-cookie"]).toBeUndefined();
 expect(registered.body).not.toContain("fixture-owner-secret");
 expect((await call("/__studio/register",{authorization:"Bearer fixture-owner-secret",origin:"https://evil.example"},"POST",body)).status).toBe(403);
});
it("supports localhost only as its own exact origin and port",async()=>{
 const host=`localhost:${port}`;
 const shell=await call("/",{host,"sec-fetch-mode":"navigate","sec-fetch-dest":"document","sec-fetch-site":"none"});
 expect(shell.status).toBe(200);
 const cookie=(shell.headers["set-cookie"] as string[])[0]!.split(";")[0]!;
 expect((await call("/__studio/session",{host,cookie,origin:`http://${host}`})).status).toBe(200);
 expect((await call("/__studio/session",{host,cookie,origin:`http://127.0.0.1:${port}`})).status).toBe(403);
 expect((await call("/",{host:"127.0.0.1:1","x-forwarded-host":`127.0.0.1:${port}`})).status).toBe(403);
});

it("rejects a foreign Host on the protected endpoint even when Origin matches it",async()=>{
 const headers=await browserHeaders();
 expect((await call("/__studio/session",{...headers,host:`evil.example:${port}`,origin:`http://evil.example:${port}`})).status).toBe(403);
});
