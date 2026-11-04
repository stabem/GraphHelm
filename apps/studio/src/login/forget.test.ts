import { afterEach, expect, it, vi } from "vitest";
import { addFolder, clearLegacyCredentials, login } from "./session";
afterEach(()=>vi.unstubAllGlobals());
it("removes old browser credentials without reading them",()=>{
 const removeItem=vi.fn(); const getItem=vi.fn(()=>{throw new Error("must not read credentials");});
 vi.stubGlobal("sessionStorage",{removeItem,getItem}); clearLegacyCredentials();
 expect(removeItem).toHaveBeenCalledWith("graphhelm.studio-next.sessionKeys"); expect(getItem).not.toHaveBeenCalled();
});
it("requests metadata without tickets and refuses unknown project instead of falling back",async()=>{
 const wanted="a".repeat(64);
 vi.stubGlobal("fetch",vi.fn(async(url:string)=>url.includes("identity") ? new Response(JSON.stringify({projectId:"b".repeat(64),projectIds:[]})) : new Response("{}",{status:404})));
 expect(await login(`?project=${wanted}&view=proof`)).toBeNull();
});
it("bounds each local session request with an abort signal", async () => {
 const id = "b".repeat(64);
 const fetcher = vi.fn(async (url: string, init: RequestInit) => {
  expect(init.signal).toBeInstanceOf(AbortSignal);
  if (url.endsWith("/__studio/identity")) return new Response(JSON.stringify({ projectId: id, projectIds: [] }));
  if (url.startsWith("/__studio/session")) return new Response(JSON.stringify({ ok: true, projectId: id }));
  return new Response(JSON.stringify({ ok: true, projectId: id }));
 });
 vi.stubGlobal("fetch", fetcher);
 expect(await login("")).toHaveLength(1);
 expect(await addFolder("/synthetic-project")).toEqual({ projectId: id });
 expect(fetcher).toHaveBeenCalledTimes(3);
});
