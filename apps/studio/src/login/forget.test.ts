import { afterEach, expect, it, vi } from "vitest";
import { clearLegacyCredentials, login } from "./session";
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
