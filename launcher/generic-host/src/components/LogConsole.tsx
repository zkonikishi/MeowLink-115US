import { memo, useEffect, useRef } from "react";
import { PanelFrame } from "./PanelFrame";
import { useLogLines } from "../useLogLines";

const MAX_ROWS = 900;

/// 没有 props：日志自己订自己拉，停靠层重排/拖动时不必跟着重画这上千行。
export default memo(function LogConsole() {
  const lines = useLogLines();
  const box = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const auto = useRef(false);

  // 只有玩家本来就看到底部时才跟着滚，往上翻历史的人不该被强行拽走。
  useEffect(() => {
    const node = box.current;
    if (!node || !pinned.current) return;
    // 自己把滚动条按到底也会派发 scroll 事件：不挂这个标记，那一帧就会把自己判成「玩家往上翻了」。
    auto.current = true;
    node.scrollTop = node.scrollHeight;
    requestAnimationFrame(() => {
      auto.current = false;
    });
  }, [lines]);

  const visible = lines.length > MAX_ROWS ? lines.slice(-MAX_ROWS) : lines;

  return (
    <PanelFrame extra={<span className="chip">{visible.length} 行</span>}>
      <div
        className="console-body"
        ref={box}
        onScroll={() => {
          if (auto.current) return;
          const node = box.current;
          if (node) {
            pinned.current = node.scrollHeight - node.scrollTop - node.clientHeight < 40;
          }
        }}
      >
        {visible.map((line) => (
          <div className="log-row" data-level={line.level} key={line.seq}>
            <time>{line.time}</time>
            <span>{line.text}</span>
          </div>
        ))}
      </div>
    </PanelFrame>
  );
});
