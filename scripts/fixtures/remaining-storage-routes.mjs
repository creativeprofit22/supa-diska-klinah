import assert from 'node:assert/strict';

// Synthetic native DTO replay, not native execution evidence. Feature DTO contracts:
// shared/storage/types.ts; features/{cleaner,browser,duplicates,empty-folders}/types.ts;
// features/uninstaller/api.ts. Every path and program below is fictional.
export function remainingPrelude() {
  return `(${installReplay.toString()})();`;
}
function installReplay() {
  const id = n => n.toString(16).padStart(32, '0');
  const state = window.__remainingFixture = {calls:[], pages:[], plans:[], mutationAttempts:0, forbidden:0, starts:[], prepared:0};
  const complete = {reasons:[]};
  const path = 'C:\\Users\\FixtureOnly\\Very long fictional personal folder for browser reflow verification';
  const file = n => ({recordId:id(n), displayPath:path+'\\Default\\Cache\\fictional-'+n+'.bin', logicalBytes:4096, allocatedBytes:null, modifiedUnixSeconds:null, eligibility:{kind:'eligible',candidate_id:id(n+100)}});
  const job = {jobId:id(900),programId:id(50),programName:'Fictional editor',state:'outcomeUnknown',createdAt:1700000000,updatedAt:1700000000,exitCode:null,launchError:null,persistenceError:false,completionMeaning:'Launcher outcome only',leftoverSupport:'unknown'};
  const modules = {start_cleaner:'cleaner',start_browser_scan:'browser',start_duplicates:'duplicates',start_empty_folders:'emptyFolders',start_program_inventory:'uninstaller'};
  window.__TAURI_INTERNALS__ = {invoke:async(command, bytes) => {
    state.calls.push(command);
    if (/execute|undo|confirm_vendor_job|purge|cancel_vendor_job/.test(command)) {state.mutationAttempts++; throw Error('Mutation forbidden in fixture replay: '+command);}
    const input = bytes instanceof Uint8Array ? JSON.parse(new TextDecoder().decode(bytes)) : bytes ?? {};
    if(command==='list_cleaner_catalog') return {targets:[{catalogId:'fixture',targetId:'cache',path:path,source:'Synthetic verification fixture',revision:'1',ruleVersion:1,minimumAgeSeconds:86400,consequence:'Cache rebuilt',exclusions:['Private data'],matcher:'files',unsupportedReason:null}],unsupportedOperations:[['recycleBin','Unsupported']]};
    if(command==='list_browser_policy') return {source:'Synthetic verification fixture',revision:'1',lifecycle:'cache',risk:'low',consequence:'Cache rebuilt',minimumAgeSeconds:86400,serviceWorkerDisclosure:'Service-worker caches may contain offline data; opt in explicitly.',unsupported:['Active or unknown browser activity'],exclusions:['Cookies','History','Logins'],profileCacheRoots:['Cache'],sharedCacheRoots:['ShaderCache']};
    if(command==='list_storage_scopes') return [{scopeId:id(10),module:input.module,label:'Fictional cache scope',displayPath:path,available:true},{scopeId:id(11),module:input.module,label:'Unavailable fictional scope',displayPath:null,available:false}];
    if(command==='choose_storage_root'||command==='authorize_storage_scope') return {rootId:id(20+state.calls.length),module:input.module,displayPath:path};
    if(modules[command]) {state.starts.push({command,input}); return id(1);}
    if(command==='storage_scan_status') return {snapshotId:id(1),module:input.module,phase:'complete',visitedEntries:6,retainedRecords:4,hashedBytes:8192,completedHashes:2,completeness:complete};
    if(command==='storage_scan_page') {
      state.pages.push(input);
      if(input.pageSize!==100) throw Error('Expected bounded page request 100');
      if(input.cursor && input.cursor!=='000000000000000000000000000003e7') throw Error('Unexpected replay cursor');
      const next = !!input.cursor;
      let records;
      if(input.collection==='duplicateGroups') records=[{kind:'duplicateGroup',record:{groupId:id(next?41:40),memberCount:3,independentCopies:3,bytesPerCopy:4096,completeness:complete}}];
      else if(input.collection==='duplicateMembers') records=(next?[3]:[1,2]).map(n=>({kind:'duplicateMember',record:{groupId:input.parentId,file:file(n)}}));
      else if(input.collection==='emptyFolders') records=[{kind:'emptyFolder',record:{recordId:id(next?4:1),displayPath:path+'\\empty-'+(next?4:1),depth:2,descendantDirectories:0,completeness:complete,eligibility:{kind:'eligible',candidate_id:id(next?104:101)}}}];
      else if(input.collection==='programs') records=[{kind:'program',record:{programId:id(next?51:50),name:next?'Fictional viewer':'Fictional editor',publisher:null,version:null,installDate:null,estimatedSizeBytes:null,lastUsedAt:null,leftoverSupport:'unknown'}}];
      else if(input.collection==='files') records=[{kind:'file',record:file(next?4:1)}];
      else throw Error('Unexpected collection '+input.collection);
      return {snapshotId:id(1),records,nextCursor:next?null:'000000000000000000000000000003e7',retainedTotal:input.collection==='duplicateMembers'?3:2,completeness:complete};
    }
    if(command==='create_storage_plan') {
      if(input.selection.snapshotId!==id(1)||input.selection.candidateIds.length!==1||![id(101),id(102),id(103)].includes(input.selection.candidateIds[0])) throw Error('Explicit selection required');
      state.plans.push(input);
      return {planId:id(800),disposition:input.disposition,selectedCount:1,selectedBytes:input.selection.module==='emptyFolders'?0:4096};
    }
    if(command==='vendor_job_history') return {records:[job],nextCursor:null};
    if(command==='prepare_vendor_job') {if(input.programId!==id(50)||input.snapshotId!==id(1)) throw Error('Wrong vendor selection');state.prepared++;return {...job,jobId:id(901),state:'awaitingConfirmation'};}
    if(command==='release_storage_scan'||command==='cancel_storage_scan'||command==='release_vendor_job') return;
    if(command==='cleanup_history') return {records:[],nextCursor:null};
    // Read-only shell/settings reads. English is pinned so assertions don't depend on the host UI language.
    if(command==='get_app_settings') return {schemaVersion:1,language:'en',updateCheck:false};
    if(command==='get_update_status') return {currentVersion:'0.1.0',configured:false,update:{state:'idle'}};
    state.forbidden++; throw Error('Unapproved native invoke in replay: '+command);
  }};
  addEventListener('DOMContentLoaded',()=>{const note=document.createElement('p');note.setAttribute('role','note');note.textContent='SYNTHETIC FIXTURE REPLAY ONLY — all native IPC intercepted; fictional paths/programs; no filesystem or vendor connection.';document.body.prepend(note);});
}

export async function runRemainingRoutes({evaluate,command,press,tabTo,until,shot,noOverflow}) {
  const tab=text=>tabTo(text,180);
  const activate=async text=>{await tab(text);await press('Enter',13);};
  const focus=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);
  const has=text=>evaluate(`document.body.textContent.includes(${JSON.stringify(text)})`);
  const mounted=selector=>until(()=>evaluate(`!!document.querySelector(${JSON.stringify(selector)})`),selector);
  const passed=[];
  for(const [route,title,scan,result] of [
    ['cleaner','Rule cleaner','Scan cleaner scope','Cleaner results'],
    ['duplicates','Duplicate files','Scan for duplicates','Duplicate groups'],
    ['empty-folders','Empty folders','Scan for empty folders','Empty folder results'],
    ['browser','Browser caches','Scan browser scope','Browser results'],
    ['uninstaller','Installed programs','Refresh inventory','Installed program results'],
  ]) {
    await command('Emulation.setDeviceMetricsOverride',{width:1280,height:1000,deviceScaleFactor:1,mobile:false});
    await evaluate(`location.hash='#/${route}'`);
    await until(()=>evaluate(`document.querySelector('h1')?.textContent===${JSON.stringify(title)}`),'actual '+route,30000);
    const hash=await evaluate('location.hash');
    await focus('.skip-link');await press('Enter',13);
    assert.equal(await evaluate('location.hash'),hash,'Skip link must not navigate hash router');
    assert.equal(await evaluate('document.activeElement.id'),'main-content','Skip link focuses main');
    if(route==='cleaner'||route==='browser') {
      await mounted('input[type=radio]:not(:disabled)');
      if(route==='browser') assert.equal(await evaluate('document.querySelector("input[type=checkbox]").checked'),false);
      await focus('input[type=radio]:not(:disabled)');await press(' ',32);
      await activate('Authorize selected scope');
    } else if(route!=='uninstaller') await activate('Choose folder');
    await activate(scan);await mounted('[aria-label="'+result+'"]');
    if(route==='duplicates') {
      assert.equal(await evaluate('document.querySelectorAll("[aria-label=\\"Duplicate groups\\"] > li").length'),1);
      await activate('Next page');await until(()=>evaluate('window.__remainingFixture.pages.at(-1).collection==="duplicateGroups" && window.__remainingFixture.pages.at(-1).cursor==="000000000000000000000000000003e7" && document.querySelector(".storage-paging button:last-child").disabled && document.querySelector(".storage-paging").getAttribute("aria-busy")==="false"'),'group page');
      await activate('First page');await activate('Review group');await mounted('[aria-label="Duplicate members"] input:disabled');
      assert.equal(await evaluate('document.querySelector("[aria-label=\\"Duplicate members\\"] input").disabled'),true);
    }
    const rows=route==='duplicates'?'[aria-label="Duplicate members"]':'[aria-label="'+result+'"]';
    assert.ok(await evaluate(`document.querySelector(${JSON.stringify(rows)}).children.length<=2`),'bounded replacement rows');
    await noOverflow();await shot(route+'-desktop-results');
    if(route==='uninstaller') {
      for(const text of ['Publisher: Unknown','Last use: Unknown','Outcome unknown:','Leftovers: unknown ownership']) assert.equal(await has(text),true,text);
      assert.equal(await evaluate('[...document.querySelectorAll("button")].some(b=>/undo|leftover/i.test(b.textContent))'),false);
      await activate('Review vendor uninstall for Fictional editor');await mounted('[aria-label="Immutable vendor job review"]');
      assert.equal(await has('Nothing has launched.'),true);
      assert.equal(await has('Vendor uninstall cannot be undone here.'),true);
    } else {
      assert.equal(await evaluate(`document.querySelectorAll(${JSON.stringify(rows+' input:checked')}).length`),0);
      await focus(rows+' input:not(:disabled)');await press(' ',32);assert.equal(await has('1 selected'),true);
      await activate('Next page');await until(()=>evaluate(`document.querySelector(${JSON.stringify(rows)}).textContent.includes(${JSON.stringify(route==='duplicates'?'fictional-3':route==='empty-folders'?'empty-4':'fictional-4')})`),'page replacement');
      assert.equal(await has('1 selected'),true);
      const review=route==='empty-folders'?'Review: Delete permanently':'Review: Move to app recovery';
      await activate(review);await mounted('dialog[open]');assert.equal(await evaluate('document.activeElement.textContent'),'Cancel review');
      await press('Tab',9,8);assert.notEqual(await evaluate('document.activeElement.textContent'),'Cancel review');
      await press('Tab',9);assert.equal(await evaluate('document.activeElement.textContent'),'Cancel review');
      await press('Enter',13);await until(()=>evaluate('!document.querySelector("dialog[open]")'),'cancel review');assert.equal(await evaluate('document.activeElement.textContent'),review);
      await press('Enter',13);await mounted('dialog[open]');
    }
    await noOverflow();await shot(route+'-desktop');
    await command('Emulation.setDeviceMetricsOverride',{width:320,height:850,deviceScaleFactor:1,mobile:false});await noOverflow();await shot(route+'-mobile');
    if(route!=='uninstaller') {
      if(route==='browser') {
        await command('Emulation.setEmulatedMedia',{features:[{name:'forced-colors',value:'active'}]});await noOverflow();await shot('browser-forced-colors');await command('Emulation.setEmulatedMedia',{features:[]});
        await command('Emulation.setDeviceMetricsOverride',{width:640,height:900,deviceScaleFactor:1,mobile:false});await evaluate('document.documentElement.style.fontSize="200%"');await noOverflow();await shot('browser-text-200');await evaluate('document.documentElement.style.fontSize=""');
      }
      await press('Escape',27);await until(()=>evaluate('!document.querySelector("dialog[open]")'),'Escape review');
      assert.equal(await evaluate('document.activeElement.textContent'),route==='empty-folders'?'Review: Delete permanently':'Review: Move to app recovery');
      await command('Emulation.setDeviceMetricsOverride',{width:320,height:850,deviceScaleFactor:1,mobile:false});
      await noOverflow();await shot(route+'-mobile-results');
      if(route==='browser') {
        await focus('.browser-page > label input');await press(' ',32);await until(()=>evaluate('!document.querySelector("[aria-label=\\"Browser results\\"]")'),'opt-in resets results');
        await activate('Authorize selected scope');await activate(scan);await mounted('[aria-label="Browser results"]');assert.equal(await has('0 selected'),true);
        assert.equal(await evaluate('window.__remainingFixture.starts.at(-1).input.serviceWorkerOptIn'),true);
        await focus('.browser-page > label input');await press(' ',32);await until(()=>evaluate('!document.querySelector("[aria-label=\\"Browser results\\"]")'),'opt-out resets results');
      }
    }
    passed.push(route+': actual route, bounded replay, desktop/mobile, keyboard review/read-only safety');
  }
  const state=await evaluate('window.__remainingFixture');assert.equal(state.mutationAttempts,0);assert.equal(state.forbidden,0);assert.equal(state.prepared,1);
  assert.equal(state.starts.find(s=>s.command==='start_browser_scan').input.serviceWorkerOptIn,false);
  return {fixture:'Synthetic injected native DTO replay, not live native evidence',passed,calls:state.calls,plans:state.plans,mutationAttempts:0,forbiddenCalls:0,notChecked:['Manual Narrator','Actual Windows dialogs/UAC','Live native execution/undo/vendor confirmation','Real caches or installed programs']};
}
