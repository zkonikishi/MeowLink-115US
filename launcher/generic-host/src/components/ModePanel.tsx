import { memo, useCallback, useEffect, useState } from "react";
import { PanelFrame } from "./PanelFrame";
import { commands, errText, type ConfigView } from "../bridge";

/// 自己读配置：停靠与浮出两条路共用同一份状态，界面层不再往下传。
export default memo(function ModePanel() {
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [working, setWorking] = useState(false);
  const [message, setMessage] = useState("");

  const load = useCallback(async () => {
    try {
      setConfig(await commands.config());
    } catch (err) {
      setMessage(errText(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const apply = async (mode: string) => {
    setWorking(true);
    setMessage("");
    try {
      await commands.setGameMode(mode);
      await load();
    } catch (err) {
      setMessage(errText(err));
    } finally {
      setWorking(false);
    }
  };

  const current = config?.game_modes.find((option) => option.value === config.game_mode);

  return (
    <PanelFrame extra={current ? <span className="chip">{current.label}</span> : null}>
      <div className="mode-body">
        <div className="segmented">
          {(config?.game_modes ?? []).map((option) => (
            <button
              key={option.value}
              aria-pressed={config?.game_mode === option.value}
              disabled={working}
              onClick={() => void apply(option.value)}
            >
              {option.label}
            </button>
          ))}
        </div>
        <p className="hint">
          {message || "切换后下次「开始游戏」生效，只影响服务端加载哪套内容，不动整合包里的文件。"}
        </p>
      </div>
    </PanelFrame>
  );
});
