import { useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CloseIcon, MaximizeIcon, MinimizeIcon, RestoreIcon, SunIcon, MoonIcon } from "./icons";
import type { Theme } from "../theme";

/** 窗口现在是不是最大化。无边框之后，图标得自己跟着状态换。 */
export function useMaximized(): [boolean, () => void] {
  const win = useMemo(() => getCurrentWindow(), []);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let alive = true;
    let stop: (() => void) | null = null;
    const sync = () => {
      void win
        .isMaximized()
        .then((value) => alive && setMaximized(value))
        .catch(() => {});
    };
    sync();
    void win.onResized(sync).then((unlisten) => {
      if (alive) stop = unlisten;
      else unlisten();
    });
    return () => {
      alive = false;
      stop?.();
    };
  }, [win]);

  return [maximized, () => void win.toggleMaximize()];
}

export default function WindowControls({
  theme,
  onToggleTheme,
}: {
  theme: Theme;
  onToggleTheme: () => void;
}) {
  const win = useMemo(() => getCurrentWindow(), []);
  const [maximized, toggleMaximize] = useMaximized();

  return (
    <div className="win-controls">
      <button
        className="win-btn"
        aria-label={theme === "dark" ? "切换到明亮模式" : "切换到黑暗模式"}
        onClick={onToggleTheme}
      >
        {theme === "dark" ? <SunIcon /> : <MoonIcon />}
      </button>
      <button className="win-btn" aria-label="最小化" onClick={() => void win.minimize()}>
        <MinimizeIcon />
      </button>
      <button
        className="win-btn"
        aria-label={maximized ? "还原" : "最大化"}
        onClick={toggleMaximize}
      >
        {maximized ? <RestoreIcon /> : <MaximizeIcon />}
      </button>
      <button
        className="win-btn"
        data-kind="close"
        aria-label="关闭"
        onClick={() => void win.close()}
      >
        <CloseIcon />
      </button>
    </div>
  );
}
