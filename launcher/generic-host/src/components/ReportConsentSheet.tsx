import { useState } from "react";

/// 第一次见到玩家时的上传范围说明：先说清楚传什么，再让人选，不替人默认打开。
export default function ReportConsentSheet({
  onChoose,
  onLater,
}: {
  onChoose: (enabled: boolean) => Promise<void>;
  onLater: () => void;
}) {
  const [pending, setPending] = useState<boolean | null>(null);

  const pick = async (enabled: boolean) => {
    setPending(enabled);
    try {
      await onChoose(enabled);
    } finally {
      setPending(null);
    }
  };

  return (
    <div className="scrim" onMouseDown={onLater}>
      <div className="sheet sheet--narrow" onMouseDown={(event) => event.stopPropagation()}>
        <header className="sheet-head">
          <h3>要不要自动上报故障</h3>
          <span className="spacer" />
        </header>
        <div className="sheet-body">
          <p className="hint">
            启动器遇到启动失败、游戏异常退出或自己内部出错时，可以把这些情况发到反馈站，方便定位。
          </p>
          <ul className="scope">
            <li>上传的是：故障步骤、标题与详情、客户端与启动器版本、系统环境、退出码，以及运行日志的尾部</li>
            <li>日志会先做大小、编码与隐私过滤</li>
          </ul>
          <ul className="scope" data-tone="never">
            <li>不上传登录令牌、密码、验证码</li>
            <li>不上传完整用户目录，也不上传整份日志</li>
            <li>正常退出不报；只有退出码与客户端日志一起判成崩溃才报</li>
          </ul>
          <div className="quit-options">
            <button
              className="quit-option"
              onClick={() => pick(true)}
              disabled={pending !== null}
            >
              <b>开启自动上报</b>
              <span>出故障时自动记一条并发往反馈站；同一异常短时间内只报一次。</span>
            </button>
            <button
              className="quit-option"
              onClick={() => pick(false)}
              disabled={pending !== null}
            >
              <b>不开启</b>
              <span>故障只留在本机，随时可在设置页改，也可以自己到「提交问题」里手写。</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
