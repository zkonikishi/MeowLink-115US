import { useState } from "react";

export type QuitChoice = "stop" | "keep";

export default function QuitSheet({
  running,
  detail,
  onChoose,
  onCancel,
}: {
  running: boolean;
  detail: string;
  onChoose: (choice: QuitChoice) => Promise<void>;
  onCancel: () => void;
}) {
  const [pending, setPending] = useState<QuitChoice | null>(null);

  const pick = async (choice: QuitChoice) => {
    setPending(choice);
    try {
      await onChoose(choice);
    } finally {
      setPending(null);
    }
  };

  return (
    <div className="scrim" onMouseDown={onCancel}>
      <div
        className="sheet sheet--narrow"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="sheet-head">
          <h3>退出启动器</h3>
          <span className="spacer" />
          <button className="btn" data-tone="ghost" onClick={onCancel}>
            返回
          </button>
        </header>
        <div className="sheet-body">
          {running ? (
            <div className="quit-options">
              <button className="quit-option" onClick={() => pick("stop")} disabled={pending !== null}>
                <b>停止环境并退出</b>
                <span>
                  结束客户端与服务端残留，等网关端口释放后再退出。
                  {detail ? `（当前：${detail}）` : ""}
                </span>
              </button>
              <button className="quit-option" onClick={() => pick("keep")} disabled={pending !== null}>
                <b>保持运行并退出</b>
                <span>游戏与服务端继续跑着，只关启动器窗口。下次进来会自动认出「运行中」。</span>
              </button>
            </div>
          ) : (
            <div className="quit-options">
              <button className="quit-option" onClick={() => pick("keep")} disabled={pending !== null}>
                <b>退出</b>
                <span>当前没有会话在运行。</span>
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
