import assert from "node:assert/strict";

export function largeFilesPrelude(data) {
  assert.deepEqual(data.pages.map(p=>p.records[0].record.logicalBytes),[9,6,4]);
  assert.equal(data.quarantinePlan.selectedBytes,9);
  return `(()=>{
    const data=${JSON.stringify(data)};
    const state=window.__largeFilesFixture={calls:[],plans:[],chooseCount:0,mutationAttempts:0,forbidden:0};
    window.__TAURI_INTERNALS__={invoke:async(command,bytes)=>{
      state.calls.push(command);
      if(/execute|undo|purge/.test(command)){state.mutationAttempts++;throw Error('Native mutation forbidden: '+command);}
      const input=JSON.parse(new TextDecoder().decode(bytes));
      if(command==='choose_storage_root')return {module:'largeFiles',rootId:(++state.chooseCount+500).toString(16).padStart(32,'0'),displayPath:data.pages[0].records[0].record.displayPath.replace(/[^\\\\]+$/,'')};
      if(command==='start_large_files'){if(input.filter.minimumBytes!==0)throw Error('Small fixture requires minimum MiB 0');return data.status.snapshotId;}
      if(command==='storage_scan_status')return structuredClone(data.status);
      if(command==='storage_scan_page'){
        if(input.pageSize!==100)throw Error('Expected UI request bound 100');
        const index=input.cursor?data.pages.findIndex(p=>p.nextCursor===input.cursor)+1:0;
        if(!data.pages[index])throw Error('Invalid replay cursor');
        return structuredClone(data.pages[index]);
      }
      if(command==='create_storage_plan'){
        if(input.selection.snapshotId!==data.status.snapshotId || input.selection.candidateIds.length!==1 || input.selection.candidateIds[0]!==data.pages[0].records[0].record.eligibility.candidate_id)throw Error('Review must retain explicit first-page selection');
        state.plans.push(structuredClone(input));
        if(input.disposition==='quarantine')return structuredClone(data.quarantinePlan);
        if(input.disposition==='permanent')return {...data.quarantinePlan,disposition:'permanent'};
        throw Error('Unsupported disposition');
      }
      if(command==='release_storage_scan'||command==='cancel_storage_scan')return;
      state.forbidden++;throw Error('Unrelated native IPC forbidden: '+command);
    }};
    addEventListener('DOMContentLoaded',()=>{const note=document.createElement('p');note.setAttribute('role','note');note.textContent='Verification replay only: actual disposable native Raw IPC, pageSize 1 below requested 100. Permanent summary is adapted for copy review only. No live filesystem or mutation connection.';document.body.prepend(note);});
  })()`;
}

export async function runLargeFilesRoute({evaluate,command,press,tabTo,until,shot,noOverflow}) {
  const tab=text=>tabTo(text,128);
  const ready=()=>until(()=>evaluate("!!document.querySelector('[aria-label=\"Large file results\"]')"),'file results');
  const selected=n=>evaluate(`document.body.textContent.includes('${n} selected (maximum')`);
  const choose=async()=>{await tab('Choose folder');await press('Enter',13);};
  const start=async()=>{await tab('Scan for large files');await press('Enter',13);await ready();};
  const select=async()=>{await evaluate("document.querySelector('.large-files-records input').focus()");await press(' ',32);assert.equal(await selected(1),true);};
  const dismiss=async(label)=>{await press('Escape',27);await until(()=>evaluate("!document.querySelector('dialog[open]')"),'Escape dismiss');assert.equal(await evaluate('document.activeElement.textContent'),label);};
  await until(()=>evaluate("document.querySelector('h1')?.textContent==='Large files'"),'actual route',30000);
  assert.equal(await evaluate("document.querySelector('a[href=\"#/large-files\"]').getAttribute('aria-current')"),'page');
  await choose();await tab('Choose folder');await press('Tab',9);
  assert.equal(await evaluate('document.activeElement.type'),'number');
  await press('a',65,2);await command('Input.insertText',{text:'0'});await start();
  assert.equal(await selected(0),true);
  assert.equal(await evaluate("document.querySelectorAll('.large-files-records input:checked').length"),0);
  await select();await noOverflow();await shot('desktop-results');
  await tab('Next page');await press('Enter',13);
  await until(()=>evaluate("document.querySelector('.large-files-records').textContent.includes('other.bin')"),'next page replacement');
  assert.equal(await evaluate("document.querySelectorAll('.large-files-records li').length"),1);
  assert.equal(await evaluate("document.querySelector('.large-files-records').textContent.includes('large.txt')"),false);
  assert.equal(await selected(1),true);
  const recovery='Review: Move to app recovery';
  await tab(recovery);await press('Enter',13);await until(()=>evaluate("!!document.querySelector('dialog[open]')"),'recovery review');
  assert.equal(await evaluate('document.activeElement.textContent'),'Cancel review');
  for(const text of ['1 item · 9 B selected, not reclaimed.','fixed to the reviewed scan and selection','does not free disk space','without automatic purge'])assert.equal(await evaluate(`document.querySelector('dialog').textContent.includes(${JSON.stringify(text)})`),true);
  await press('Tab',9,8);assert.equal(await evaluate('document.activeElement.textContent'),'Move to app recovery');
  await press('Tab',9);assert.equal(await evaluate('document.activeElement.textContent'),'Cancel review');
  await shot('desktop-recovery');await dismiss(recovery);
  await command('Emulation.setDeviceMetricsOverride',{width:320,height:850,deviceScaleFactor:1,mobile:false});
  await noOverflow();await shot('mobile-results');
  await press('Enter',13);await until(()=>evaluate("!!document.querySelector('dialog[open]')"),'mobile recovery');await noOverflow();await shot('mobile-recovery');
  await command('Emulation.setEmulatedMedia',{features:[{name:'forced-colors',value:'active'}]});await noOverflow();await shot('forced-colors-review');
  await command('Emulation.setEmulatedMedia',{features:[]});
  await command('Emulation.setDeviceMetricsOverride',{width:640,height:900,deviceScaleFactor:1,mobile:false});await evaluate("document.documentElement.style.fontSize='200%'");await noOverflow();await shot('text-200-percent-review');await dismiss(recovery);
  await evaluate("document.documentElement.style.fontSize=''");
  const permanent='Review: Delete permanently';await tab(permanent);await press('Enter',13);await until(()=>evaluate("!!document.querySelector('dialog[open]')"),'permanent copy review');
  assert.equal(await evaluate("document.querySelector('dialog').textContent.includes('This cannot be undone. Continuing opens a separate Windows confirmation.')"),true);
  await press('Tab',9);assert.equal(await evaluate('document.activeElement.textContent'),'Continue to Windows confirmation');await shot('permanent-copy-only');await dismiss(permanent);
  // Filter and scope reset must remove results and disable review, not retain IDs.
  await evaluate("document.querySelector('.large-files-controls input').focus()");await press('a',65,2);await command('Input.insertText',{text:'1'});
  await until(()=>evaluate("!document.querySelector('.large-files-records')"),'filter reset');
  assert.equal(await evaluate("[...document.querySelectorAll('button')].find(b=>b.textContent==='Review: Move to app recovery').disabled"),true);
  await press('a',65,2);await command('Input.insertText',{text:'0'});await choose();await start();assert.equal(await selected(0),true);await select();
  await choose();await until(()=>evaluate("!document.querySelector('.large-files-records')"),'scope reset');await start();assert.equal(await selected(0),true);
  assert.equal(await evaluate('window.__largeFilesFixture.mutationAttempts'),0);assert.equal(await evaluate('window.__largeFilesFixture.forbidden'),0);
  assert.equal(await evaluate("window.__largeFilesFixture.plans.map(p=>p.disposition).join(',')"),'quarantine,quarantine,permanent');
  return {passed:['actual hash route and navigation','native 9/6/4-byte bounded-page replay, minimum MiB 0','explicit selection, next-page replacement with retained IDs','immutable app recovery review, keyboard Tab/Space/Enter/Escape and focus return','permanent warning copy only','scope/filter/new-scan reset','1280px desktop, 320px mobile, forced colors, 200% text reflow'],mutationAttempts:0,forbiddenCalls:0,notChecked:['live picker','native permanent confirmation UI or execution','Narrator','permanent summary is adapted from actual quarantine summary; not native permanent evidence']};
}
