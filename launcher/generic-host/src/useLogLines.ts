import { useEffect, useState } from "react";
import { commands, events, type LogLine } from "./bridge";

/** 谁要日志谁自己订：停靠面板与浮出的独立窗口共用这一条路径，不做跨窗口同步。 */
export function useLogLines(limit = 600): LogLine[] {
  const [lines, setLines] = useState<LogLine[]>([]);

  useEffect(() => {
    let alive = true;
    const stops: Array<() => void> = [];

    const merge = (incoming: LogLine[]) =>
      setLines((prev) => {
        const seen = new Set(prev.map((line) => line.seq));
        const extra = incoming.filter((line) => !seen.has(line.seq));
        if (extra.length === 0) return prev;
        return [...prev, ...extra].sort((a, b) => a.seq - b.seq);
      });

    // 先挂监听再取快照：反过来会把中间那几行彻底漏掉。
    void events.onLog((line) => merge([line])).then((stop) => {
      if (alive) stops.push(stop);
      else stop();
      return commands.logSnapshot(limit);
    }).then((snapshot) => {
      if (alive) merge(snapshot);
    });

    return () => {
      alive = false;
      stops.forEach((stop) => stop());
    };
  }, [limit]);

  return lines;
}
