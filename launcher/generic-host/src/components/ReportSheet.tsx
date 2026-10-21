import type { ReactNode } from "react";
import type { HealthReport, PortReport } from "../bridge";

const MARK: Record<string, string> = { ok: "✓", warn: "!", error: "✕" };

function Sheet({
  title,
  subtitle,
  children,
  footer,
  onClose,
}: {
  title: string;
  subtitle?: string;
  children: ReactNode;
  footer: ReactNode;
  onClose: () => void;
}) {
  return (
    <div className="scrim" onMouseDown={onClose}>
      <div className="sheet" onMouseDown={(event) => event.stopPropagation()}>
        <header className="sheet-head">
          <h3>{title}</h3>
          {subtitle ? <span className="chip">{subtitle}</span> : null}
          <span className="spacer" />
          <button className="btn" data-tone="ghost" onClick={onClose}>
            关闭
          </button>
        </header>
        <div className="sheet-body">{children}</div>
        <footer className="sheet-foot">{footer}</footer>
      </div>
    </div>
  );
}

export function HealthSheet({
  report,
  onClose,
  onRecheck,
}: {
  report: HealthReport | null;
  onClose: () => void;
  onRecheck: () => void;
}) {
  return (
    <Sheet
      title="环境体检"
      subtitle={report ? `${report.ok} 通过 · ${report.warn} 警告 · ${report.error} 错误` : "尚未体检"}
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={onRecheck}>
            重新体检
          </button>
          <button className="btn" data-tone="accent" onClick={onClose}>
            知道了
          </button>
        </>
      }
    >
      {report === null ? (
        <p className="hint">正在检查……</p>
      ) : (
        report.items.map((item) => (
          <div className="health-row" data-level={item.level} key={item.id}>
            <span className="mark">{MARK[item.level]}</span>
            <div>
              <div className="health-name">{item.name}</div>
              <div className="health-value">{item.value}</div>
              {item.hint ? <div className="hint">{item.hint}</div> : null}
            </div>
          </div>
        ))
      )}
    </Sheet>
  );
}

export function PortsSheet({
  report,
  hint,
  busy,
  onClose,
  onRecheck,
  onForceClean,
}: {
  report: PortReport | null;
  hint: string;
  busy: boolean;
  onClose: () => void;
  onRecheck: () => void;
  onForceClean: () => void;
}) {
  const gateway = report?.gateway;
  return (
    <Sheet
      title="端口与残留进程"
      subtitle={report ? report.summary : "正在检查……"}
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={onRecheck} disabled={busy}>
            重新检查
          </button>
          <button
            className="btn"
            data-tone="danger"
            onClick={onForceClean}
            disabled={busy || (!report || (report.blocked.length === 0 && report.leftovers.length === 0))}
            title={report && report.blocked.length === 0 && report.leftovers.length === 0 ? "没有需要清理的残留" : ""}
          >
            强制清理并继续
          </button>
        </>
      }
    >
      {report ? (
        <>
          <div className="health-row" data-level={gateway?.listening ? "error" : "ok"}>
            <span className="mark">{gateway?.listening ? "✕" : "✓"}</span>
            <div>
              <div className="health-name">
                网关端口 {gateway?.port}（{gateway?.port_label}）
              </div>
              <div className="health-value">
                {gateway?.listening
                  ? `${gateway.image ?? "名字读不到（可能需要管理员权限）"} · PID ${gateway.pid ?? "?"} · ${gateway.kind_label}`
                  : "空闲，可以开始游戏"}
              </div>
            </div>
          </div>
          {report.leftovers.length > 0 ? (
            <div className="health-row" data-level="warn">
              <span className="mark">!</span>
              <div>
                <div className="health-name">本项目残留进程 {report.leftovers.length} 个</div>
                <div className="health-value">
                  {report.leftovers.map((one) => `${one.image} · PID ${one.pid}`).join("　")}
                </div>
              </div>
            </div>
          ) : null}
          {report.protected_ones.length > 0 ? (
            <div className="health-row" data-level="warn">
              <span className="mark">!</span>
              <div>
                <div className="health-name">启动器不会自动结束的占用者</div>
                <div className="health-value">
                  {report.protected_ones
                    .map(
                      (one) =>
                        `${one.image ?? "名字读不到"} · PID ${one.pid ?? "?"}（${one.kind_label}）`,
                    )
                    .join("　")}
                </div>
              </div>
            </div>
          ) : null}
          {report.failed.length > 0 ? (
            <div className="health-row" data-level="error">
              <span className="mark">✕</span>
              <div>
                <div className="health-name">清理失败</div>
                <div className="health-value">{report.failed.join("　")}</div>
              </div>
            </div>
          ) : null}
          {hint ? <p className="hint">{hint}</p> : null}
        </>
      ) : (
        <p className="hint">正在检查……</p>
      )}
    </Sheet>
  );
}
