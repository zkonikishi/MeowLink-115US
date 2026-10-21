import { useEffect, useState } from "react";
import { commands, errText, type Phase } from "../bridge";

export default function LauncherMods({phase,onNotice}:{phase:Phase;onNotice:(text:string,tone?:"ok"|"err")=>void}) {
 const [rows,setRows]=useState<Awaited<ReturnType<typeof commands.launcherModList>>>([]);
 const [preview,setPreview]=useState<Awaited<ReturnType<typeof commands.launcherModPick>>>(null);
 const [frame,setFrame]=useState<{id:string;url:string}|null>(null);const [busy,setBusy]=useState(false);
 const reload=async()=>{const rows=await commands.launcherModList();setRows(rows);setFrame(old=>old&&rows.some(row=>row.id===old.id&&row.running)?old:null)};
 useEffect(()=>{void reload().catch(()=>{});const timer=setInterval(()=>{if(!busy)void reload().catch(()=>{})},2500);return()=>clearInterval(timer)},[busy]);
 const action=async(fn:()=>Promise<unknown>)=>{if(busy)return;setBusy(true);try{await fn();await reload()}catch(error){onNotice(errText(error),"err")}finally{setBusy(false)}};
 const localRunning=["starting","stopping"].includes(phase)||(phase==="running"&&!rows.some(row=>row.running));
 return <section className="card" style={{gridColumn:"1 / -1"}}><h2>启动器 Mod</h2>
 <p className="hint">独立业务包提供配置页面；启动器只管理导入、受控进程和版本。导入后不会自动执行。</p>
 <div className="row"><button className="btn" disabled={busy||rows.some(row=>row.running)} onClick={()=>void action(async()=>onNotice((await commands.launcherModLocal()).message))}>切回本地模式</button><button className="btn" disabled={busy} onClick={()=>void action(async()=>setPreview(await commands.launcherModPick()))}>导入 / 更新 Mod…</button><button className="btn" disabled={busy} onClick={()=>void action(reload)}>重新扫描</button></div>
 {preview?<div className="field"><strong>{preview.preview.manifest.name} · {preview.preview.manifest.version}</strong><p className="hint">作者：{preview.preview.manifest.author} · 能力：{preview.preview.manifest.capabilities.join("、")}</p><p className="hint">此包未签名。确认表示你已核对来源并信任其程序；能力声明不等同于系统沙箱。</p><p className="path">SHA256：{preview.preview.manifestSha256}</p><div className="row"><button className="btn" disabled={busy} onClick={()=>void action(async()=>{const result=await commands.launcherModApprove(preview.ticket,preview.preview.manifestSha256);setPreview(null);onNotice(result.message)})}>确认摘要并导入</button><button className="btn" disabled={busy} onClick={()=>setPreview(null)}>取消</button></div></div>:null}
 {rows.length===0?<p className="hint">还没有启动器 Mod。服务端 Lua、Go Mod 和客户端 DLL 仍使用原有入口。</p>:rows.map(row=><div className="field" key={row.id}><strong>{row.name} {row.version}</strong><p className="hint">{row.author} · {row.running?"配置进程运行中":"已停止"}{row.previousVersion?` · 可回滚 ${row.previousVersion}`:""}</p>{row.problem?<p className="hint" data-tone="err">{row.problem}</p>:null}<div className="row"><button className="btn" disabled={busy||localRunning||!!row.problem} onClick={()=>void action(async()=>setFrame({id:row.id,url:await commands.launcherModOpen(row.id)}))}>配置设置</button><button className="btn" disabled={busy||!row.running} onClick={()=>void action(async()=>{await commands.launcherModStop(row.id);setFrame(null)})}>停止实例</button><button className="btn" disabled={busy||row.running||!row.previousVersion||!!row.problem} onClick={()=>void action(async()=>onNotice((await commands.launcherModRollback(row.id)).message))}>回滚上版</button><button className="btn" disabled={busy||row.running||!!row.problem} onClick={()=>{if(window.confirm(`卸载 ${row.name} 的程序文件？账号配置将保留。`))void action(async()=>onNotice((await commands.launcherModUninstall(row.id)).message))}}>卸载</button></div></div>)}
 {frame?<div style={{marginTop:18}}><div className="row"><strong>Mod 配置页面</strong><button className="btn" onClick={()=>setFrame(null)}>收起页面（保持实例）</button></div><iframe title="启动器 Mod 配置" src={frame.url} sandbox="allow-scripts allow-same-origin" referrerPolicy="no-referrer" style={{width:"100%",height:760,border:"1px solid #465060",borderRadius:10,marginTop:12}}/></div>:null}
 </section>
}


