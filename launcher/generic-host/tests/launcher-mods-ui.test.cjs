const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const test = require('node:test');
const assert = require('node:assert/strict');
const ts = require('typescript');
const code = ts.transpileModule(fs.readFileSync(path.join(__dirname,'../src/components/LauncherMods.tsx'),'utf8'), {compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022}}).outputText;
const row = {id:'sample-mod',name:'Sample',version:'1.0.0',author:'operator',capabilities:['settings.http'],running:false,previousVersion:'0.9.0',problem:null};
function harness(options={}) {
 const state=[options.rows??[row],options.preview??null,options.frame??null,false];let index=0;const calls=[];const notices=[];
 const commands={launcherModList:async()=>state[0],launcherModPick:async()=>({ticket:'picked-ticket',preview:{manifest:{...row},manifestSha256:'a'.repeat(64)}})};
 for(const name of ['launcherModApprove','launcherModOpen','launcherModStop','launcherModRollback','launcherModUninstall','launcherModLocal'])commands[name]=async(...args)=>{calls.push([name,...args]);if(name==="launcherModOpen")state[0]=state[0].map(row=>({...row,running:row.id===args[0]}));return name==='launcherModOpen'?'http://127.0.0.1:12345/':{message:'done'}};
 const jsx=(type,props)=>({type,props});const module={exports:{}};
 const requireMock=name=>name==='react'?{useState(initial){const slot=index++;return[state[slot]??initial,value=>{state[slot]=typeof value==='function'?value(state[slot]):value}]},useEffect(){}}:name==='react/jsx-runtime'?{jsx,jsxs:jsx}:name==='../bridge'?{commands,errText:e=>e.message}:require(name);
 vm.runInNewContext(code,{exports:module.exports,module,require:requireMock,window:{confirm:()=>true},setInterval:()=>0,clearInterval(){}});
 const tree=module.exports.default({phase:options.phase??'idle',onNotice:(...args)=>notices.push(args)});
 const all=[];function walk(node){if(!node||typeof node!=='object')return;if(Array.isArray(node)){node.forEach(walk);return}all.push(node);walk(node.props?.children)}walk(tree);
 const text=node=>Array.isArray(node)?node.map(text).join(''):node&&typeof node==='object'?text(node.props?.children):String(node??'');
 return {state,calls,notices,all,button:label=>all.find(node=>node.type==='button'&&text(node).includes(label)),text:text(tree)};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
test('import requires a separate digest confirmation before any install',async()=>{const h=harness();h.button('导入 / 更新').props.onClick();await settle();await settle();assert.equal(h.state[1].ticket,'picked-ticket');assert.equal(h.calls.length,0)});
test('approval passes only the frozen ticket and digest',async()=>{const preview={ticket:'approved-ticket',preview:{manifest:row,manifestSha256:'b'.repeat(64)}};const h=harness({preview});assert.match(h.text,/未签名/);h.button('确认摘要').props.onClick();await settle();await settle();assert.equal(JSON.stringify(h.calls[0]),JSON.stringify(['launcherModApprove','approved-ticket','b'.repeat(64)]));assert.equal(h.state[1],null)});
test('configuration is rendered as a sandboxed cross-origin frame',()=>{const h=harness({rows:[{...row,running:true}],frame:{id:row.id,url:'http://127.0.0.1:12345/'},phase:'running'});const frame=h.all.find(node=>node.type==='iframe');assert.equal(frame.props.src,'http://127.0.0.1:12345/');assert.equal(frame.props.sandbox,'allow-scripts allow-same-origin');assert.equal(frame.props.referrerPolicy,'no-referrer');assert.equal(h.button('配置设置').props.disabled,false);assert.equal(h.button('回滚上版').props.disabled,true);assert.equal(h.button('卸载').props.disabled,true)});
test('original local session prevents opening a new Mod process',()=>{const h=harness({phase:'running'});assert.equal(h.button('配置设置').props.disabled,true)});
test('reopening settings uses the package ID and host-provided endpoint',async()=>{const h=harness();h.button('配置设置').props.onClick();await settle();await settle();assert.equal(JSON.stringify(h.calls[0]),JSON.stringify(['launcherModOpen','sample-mod']));assert.equal(h.state[2].url,'http://127.0.0.1:12345/')});

