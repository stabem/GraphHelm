import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { AddFolder } from "./Sidebar";
it("adds a project by folder without asking for a ticket",()=>{
 const html=renderToStaticMarkup(<AddFolder onAdd={async()=>null} open setOpen={()=>undefined}/>);
 expect(html).toContain('aria-label="Project folder"'); expect(html).not.toContain('type="password"'); expect(html).not.toContain('placeholder="ticket"');
});
