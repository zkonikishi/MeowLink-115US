"""Package the standalone, cold-start-only UI plugin; no installation."""
import hashlib,json,pathlib,sys,zipfile
sys.stdout.reconfigure(encoding='utf-8')
root=pathlib.Path(__file__).resolve().parent
data=(root/'dist/LegionPartyHUD.dll').read_bytes()
manifest={'schema':2,'id':'qol.legion-party-hud','version':'0.1.0','name':'军团隐藏纯空队伍框','author':'local','description':'保留真实队员与断线保留位，仅隐藏纯空锁框；待实机确认。','permissions':['client.file.write'],'requires':['qol.client-host'],'layers':{'client':{'ops':[{'kind':'file.add','target':'.115us-mods/LegionPartyHUD.dll','source':'client/LegionPartyHUD.dll','size':len(data),'sha256':hashlib.sha256(data).hexdigest(),'note':'仅军团HUD的一个绘制调用点，五处指令校验，冷启动加载；不修改名册与血量。'}]}}}
out=root/'dist/legion-party-hud-0.1.0.zip'
with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as z:
 z.writestr('mod.json',json.dumps(manifest,ensure_ascii=False,indent=2))
 z.writestr('client/LegionPartyHUD.dll',data)
 z.write(root/'README.md','README.md')
print(json.dumps({'package':str(out),'sha256':hashlib.sha256(out.read_bytes()).hexdigest(),'dll_sha256':hashlib.sha256(data).hexdigest()},ensure_ascii=False))
