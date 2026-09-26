import assert from "node:assert/strict";

export function analyzerPrelude(data) {
  assert.equal(data.root.records[0].record.logicalBytes,222);
  assert.equal(data.children.reduce((n,page)=>n+page.records.length,0),205);
  return `(()=>{
    const data=${JSON.stringify(data)};
    const state=window.__analyzerFixture={chooseCount:0,cancelPicker:false,slow:false,partial:false,calls:[],forbidden:0};
    const root=data.root.records[0].record;
    const clone=value=>structuredClone(value);
    window.__TAURI_INTERNALS__={invoke:async(command,bytes)=>{
      const input=JSON.parse(new TextDecoder().decode(bytes));
      state.calls.push(command);
      if(command==='choose_storage_root') {
        if(state.cancelPicker)return null;
        return {module:'diskAnalyzer',rootId:(++state.chooseCount+500).toString(16).padStart(32,'0'),displayPath:root.displayPath};
      }
      if(command==='start_disk_analyzer'){if(input.displayedDepth!==data.displayedDepth)throw Error('Fixture depth mismatch');return data.root.snapshotId;}
      if(command==='storage_scan_status'){const status=clone(data.status);status.phase=state.slow?'walking':'complete';if(state.partial)status.completeness.reasons=['entryLimit'];return status;}
      if(command==='storage_scan_page') {
        let page;
        if(input.collection==='extensions')page=clone(data.extensions);
        else if(!input.parentId)page=clone(data.root);
        else if(input.parentId===root.nodeId) {
          const index=input.cursor?data.children.findIndex(p=>p.nextCursor===input.cursor)+1:0;
          page=clone(data.children[index]);
        } else page=clone(data.leaf);
        if(!page)throw {code:'invalid_cursor'};
        if(state.partial){page.completeness.reasons=['entryLimit'];for(const row of page.records)row.record.completeness.reasons=['entryLimit'];}
        return page;
      }
      if(command==='release_storage_scan'||command==='cancel_storage_scan')return;
      state.forbidden++;throw Error('Native mutation or unrelated IPC forbidden: '+command);
    }};
    addEventListener('DOMContentLoaded',()=>{const note=document.createElement('p');note.textContent='Verification only: replaying disposable native fixture data. No live filesystem or cleanup connection.';note.setAttribute('role','note');document.body.prepend(note);});
  })()`;
}

export async function runAnalyzerRoute({evaluate,command,press,tabTo,until,shot,noOverflow}) {
  const tab = text=>tabTo(text,256);
  const totals = ()=>until(()=>evaluate(`!!document.querySelector('[aria-label="Scanned root totals"]')`),"root totals");
  const folders = ()=>until(()=>evaluate(`document.querySelectorAll('.analyzer-folder').length === 100`),"100-row native page");
  await until(()=>evaluate("document.querySelector('h1')?.textContent === 'Disk analyzer'"),"actual analyzer route mount",30000);
  await tab("Choose folder");await press("Enter",13);
  await until(()=>evaluate("!document.querySelector('.analyzer-controls button:last-child').disabled"),"chosen fixture root");
  await tab("Choose folder");await press("Tab",9);
  assert.equal(await evaluate("document.activeElement.type"),"number");
  await press("a",65,2);await command("Input.insertText",{text:"1"});
  assert.equal(await evaluate("document.activeElement.value"),"1");
  await tab("Analyze folder");await press("Enter",13);await totals();await folders();
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Scanned root totals\"]').textContent.includes('222 B')"),true);
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Scanned root totals\"]').textContent.includes('207')"),true);
  assert.equal(await evaluate("document.querySelectorAll('.analyzer-page input[type=checkbox]').length"),0);
  assert.equal(await evaluate("[...document.querySelectorAll('.analyzer-page button')].some(b=>/delete|cleanup|review|undo/i.test(b.textContent))"),false);
  await noOverflow();await evaluate("window.scrollTo(0,0)");await shot("desktop-analyzer");
  await evaluate("document.querySelector('.analyzer-page h2[tabindex]').scrollIntoView({block:'start'})");await shot("desktop-folder-results");
  await tab("d000");await press("Enter",13);
  await until(()=>evaluate("document.body.textContent.includes('No child folders within the displayed depth')"),"depth-limited child view");
  assert.equal(await evaluate("document.activeElement.textContent"),"Child folders of d000");
  assert.equal(await evaluate("document.body.textContent.includes('Current subtree: 8 B logical')"),true);
  await tab("Scanned root");await press("Enter",13);await folders();
  await tab("Next page");await press("Enter",13);
  await until(()=>evaluate("document.querySelector('.analyzer-folder')?.textContent === 'd100'"),"native second page");
  assert.equal(await evaluate("document.querySelectorAll('.analyzer-folder').length"),100);
  await tab("Next page");await press("Enter",13);
  await until(()=>evaluate("document.querySelector('.analyzer-folder')?.textContent === 'd200'"),"native final page");
  assert.equal(await evaluate("document.querySelectorAll('.analyzer-folder').length"),5);
  await tab("Extensions");await press("Enter",13);
  await until(()=>evaluate("document.querySelector('[aria-label=\"File extensions\"]')?.textContent.includes('.dat')"),"native extensions");
  await noOverflow();await evaluate("document.querySelector('.analyzer-page h2[tabindex]').scrollIntoView({block:'start'})");await shot("desktop-extensions");
  await command("Emulation.setDeviceMetricsOverride",{width:320,height:850,deviceScaleFactor:1,mobile:false});
  await noOverflow();await evaluate("window.scrollTo(0,0)");await shot("mobile-analyzer");
  await tab("Folders");await press("Enter",13);await folders();
  await tab("d000");await press("Enter",13);
  await until(()=>evaluate("document.activeElement.textContent === 'Child folders of d000'"),"mobile breadcrumb focus");
  await noOverflow();await shot("mobile-child-keyboard");
  await evaluate("window.__analyzerFixture.cancelPicker=true");
  await tab("Choose folder");await press("Enter",13);await totals();
  assert.equal(await evaluate("document.body.textContent.includes('Current subtree: 8 B logical')"),true,"picker cancellation preserves context");
  await evaluate("window.__analyzerFixture.cancelPicker=false;window.__analyzerFixture.slow=true");
  await tab("Choose folder");await press("Enter",13);
  await tab("Analyze folder");await press("Enter",13);
  await until(()=>evaluate("document.body.textContent.includes('Scanning. No files')"),"scan running");
  await tab("Cancel scan");await press("Enter",13);
  await until(()=>evaluate("document.body.textContent.includes('Scan cancelled.')"),"scan cancellation");
  assert.equal(await evaluate("!!document.querySelector('[aria-label=\"Scanned root totals\"]')"),false);
  await shot("mobile-cancelled");
  await evaluate("window.__analyzerFixture.slow=false;window.__analyzerFixture.partial=true");
  await tab("Choose folder");await press("Enter",13);await tab("Analyze folder");await press("Enter",13);await totals();await folders();
  assert.equal(await evaluate("document.body.textContent.includes('Incomplete totals:')"),true);
  await command("Emulation.setEmulatedMedia",{features:[{name:"forced-colors",value:"active"}]});
  await noOverflow();await evaluate("window.scrollTo(0,0)");await shot("forced-colors-partial");
  await command("Emulation.setEmulatedMedia",{features:[]});
  await command("Emulation.setDeviceMetricsOverride",{width:640,height:900,deviceScaleFactor:1,mobile:false});
  await evaluate("document.documentElement.style.fontSize='200%'");await noOverflow();await shot("text-200-percent");
  assert.equal(await evaluate("window.__analyzerFixture.forbidden"),0);
  return {passed:["actual hash route with disposable native fixture output","222-byte full subtree, 207 independent files","100/100/5 folder pages and global extension totals","keyboard traversal and breadcrumb focus on desktop/mobile","picker cancellation preserves existing context","scan cancellation removes results","partial warnings, forced colors, 200% text, 320px reflow"],forbiddenCalls:0,notChecked:["live native picker on this route","Narrator","large-file workflow"]};
}
