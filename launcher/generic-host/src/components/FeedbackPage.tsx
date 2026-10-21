import { useCallback, useEffect, useState } from "react";
import {
  commands,
  errText,
  type Board,
  type FeedbackMeta,
  type Issue,
  type Mine,
  type Submission,
  type SubmitOutcome,
} from "../bridge";

export type FeedbackTab = "board" | "submit" | "mine";

const TABS: Array<{ key: FeedbackTab; label: string }> = [
  { key: "board", label: "问题看板" },
  { key: "submit", label: "提交问题" },
  { key: "mine", label: "我的反馈" },
];

const EMPTY: Submission = {
  title: "",
  character: "",
  steps: "",
  expected: "",
  actual: "",
  occurred: "",
  category: "启动器与更新",
  severity: "影响正常游玩",
  frequency: "偶尔出现",
  location: "",
  contact: "",
  related_issue_id: "",
};

/** 站上的 created 是 unix 秒，网页固定按东八区显示，这里保持同一口径。 */
function dateText(value: number): string {
  if (!value) return "—";
  return new Date(value * 1000).toLocaleString("zh-CN", {
    timeZone: "Asia/Shanghai",
    hour12: false,
  });
}

/** 站上把不带时区的值当东八区时间，所以送出去的也是东八区的墙上时间。 */
function nowMinute(): string {
  return new Date(Date.now() + 8 * 3600_000).toISOString().slice(0, 16);
}

const PAGE_SIZE = 24;

export default function FeedbackPage({
  tab,
  onTab,
  onNotice,
}: {
  tab: FeedbackTab;
  onTab: (tab: FeedbackTab) => void;
  onNotice: (text: string, tone?: "ok" | "err") => void;
}) {
  const [meta, setMeta] = useState<FeedbackMeta | null>(null);
  const [related, setRelated] = useState<Issue | null>(null);

  useEffect(() => {
    let alive = true;
    commands
      .feedbackMeta()
      .then((value) => alive && setMeta(value))
      .catch(() => {
        // 拿不到枚举只是少几个下拉选项，站上的接口照样能读；不打断这一页。
      });
    return () => {
      alive = false;
    };
  }, []);

  /** 从看板的某条问题过来：带上 related_issue_id，站上会把它挂到同一条问题下。 */
  const reportSame = (issue: Issue) => {
    setRelated(issue);
    onTab("submit");
  };

  return (
    <div className="fb">
      <header className="fb-bar">
        <div className="segmented">
          {TABS.map((item) => (
            <button
              key={item.key}
              aria-pressed={tab === item.key}
              onClick={() => onTab(item.key)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <div className="fb-ver">
          {meta ? `启动器 ${meta.launcher_version} · 客户端 ${meta.client_version}` : "…"}
        </div>
      </header>

      {tab === "board" ? <BoardTab onSame={reportSame} onNotice={onNotice} /> : null}
      {tab === "submit" ? (
        <SubmitTab
          meta={meta}
          related={related}
          onClearRelated={() => setRelated(null)}
          onNotice={onNotice}
          onGoMine={() => onTab("mine")}
        />
      ) : null}
      {tab === "mine" ? <MineTab onGoSubmit={() => onTab("submit")} /> : null}
    </div>
  );
}

function BoardTab({
  onSame,
  onNotice,
}: {
  onSame: (issue: Issue) => void;
  onNotice: (text: string, tone?: "ok" | "err") => void;
}) {
  const [board, setBoard] = useState<Board | null>(null);
  const [detail, setDetail] = useState<Issue | null>(null);
  const [page, setPage] = useState(1);
  const [category, setCategory] = useState("");
  const [status, setStatus] = useState("");
  const [sort, setSort] = useState("");
  const [term, setTerm] = useState("");
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      setBoard(await commands.feedbackBoard({ page, category, status, q, sort }));
    } catch (err) {
      setError(errText(err));
      setBoard(null);
    } finally {
      setBusy(false);
    }
  }, [page, category, status, q, sort]);

  useEffect(() => {
    void load();
  }, [load]);

  const open = async (id: string) => {
    try {
      setDetail(await commands.feedbackIssue(id));
    } catch (err) {
      onNotice(errText(err), "err");
    }
  };

  const clearFilters = () => {
    setCategory("");
    setStatus("");
    setTerm("");
    setQ("");
    setPage(1);
  };

  const filtered = Boolean(category || status || q);
  const pages = board ? Math.max(1, Math.ceil(board.count / PAGE_SIZE)) : 1;

  return (
    <div className="fb-split">
      <section className="card fb-list">
        <div className="fb-tools">
          <input
            type="text"
            value={term}
            placeholder="按标题或问题编号搜"
            onChange={(e) => setTerm(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                setPage(1);
                setQ(term.trim());
              }
            }}
          />
          <button
            className="btn"
            onClick={() => {
              setPage(1);
              setQ(term.trim());
            }}
          >
            搜索
          </button>
          <button className="btn" data-tone="ghost" disabled={!filtered} onClick={clearFilters}>
            清除筛选
          </button>
          <div className="segmented fb-sort">
            <button aria-pressed={sort === ""} onClick={() => setSort("")}>
              最新
            </button>
            <button aria-pressed={sort === "popular"} onClick={() => setSort("popular")}>
              最热
            </button>
          </div>
        </div>

        <Facets
          title="状态"
          facets={board?.status_counts ?? []}
          active={status}
          onPick={(key) => {
            setStatus(key);
            setPage(1);
          }}
        />
        <Facets
          title="类型"
          facets={board?.category_counts ?? []}
          active={category}
          onPick={(key) => {
            setCategory(key);
            setPage(1);
          }}
        />

        <p className="hint">
          {busy
            ? "正在读站上公开的问题…"
            : board
              ? `共 ${board.count} 条问题 · 累计 ${board.report_count} 份反馈 · 第 ${board.page}/${pages} 页`
              : error || "还没有内容"}
        </p>

        {error ? <p className="fb-error">{error}</p> : null}

        <div className="issue-list">
          {board?.issues.map((issue) => (
            <button
              className="issue-item"
              key={issue.id}
              data-on={detail?.id === issue.id ? "true" : "false"}
              onClick={() => void open(issue.id)}
            >
              <b>{issue.title}</b>
              <div className="issue-meta">
                <span className="chip">{issue.category}</span>
                <span className="chip" data-tone={tone(issue.status)}>
                  {issue.status}
                </span>
                <span>{issue.report_count} 份反馈</span>
                <span>{dateText(issue.updated)}</span>
              </div>
              {issue.public_note ? <p className="issue-note">{issue.public_note}</p> : null}
            </button>
          ))}
          {board && board.issues.length === 0 ? (
            <p className="hint">这个筛选条件下还没有问题。换个条件，或者自己提一条。</p>
          ) : null}
        </div>

        <div className="row fb-pager">
          <button className="btn" disabled={page <= 1 || busy} onClick={() => setPage(page - 1)}>
            上一页
          </button>
          <span className="hint">
            {page} / {pages}
          </span>
          <button
            className="btn"
            disabled={page >= pages || busy}
            onClick={() => setPage(page + 1)}
          >
            下一页
          </button>
        </div>
      </section>

      <section className="card fb-detail">
        {detail ? (
          <>
            <h2>{detail.title}</h2>
            <div className="issue-meta">
              <span className="chip">{detail.category}</span>
              <span className="chip" data-tone={tone(detail.status)}>
                {detail.status}
              </span>
              <span>编号 {detail.id}</span>
              <span>{detail.report_count} 份反馈</span>
            </div>
            <p className="hint">
              首次报告 {dateText(detail.created)} · 最近更新 {dateText(detail.updated)}
            </p>
            <div className="fb-block">
              <label>处理说明</label>
              <p>{detail.public_note || "管理员还没有公开处理说明。"}</p>
            </div>
            {detail.fix_version ? (
              <div className="fb-block">
                <label>修复版本</label>
                <p>{detail.fix_version}</p>
              </div>
            ) : null}
            <div className="fb-block">
              <label>时间线</label>
              {detail.updates.length === 0 ? (
                <p className="hint">还没有处理记录。</p>
              ) : (
                <ol className="timeline">
                  {detail.updates.map((update, index) => (
                    <li key={`${update.created}-${index}`}>
                      <span className="timeline-when">
                        {dateText(update.created)} · {update.status}
                      </span>
                      <p>{update.message}</p>
                      {update.fix_version ? <em>修复版本 {update.fix_version}</em> : null}
                    </li>
                  ))}
                </ol>
              )}
            </div>
            <button className="btn" data-tone="accent" onClick={() => onSame(detail)}>
              我也遇到了，同样提一份
            </button>
          </>
        ) : (
          <p className="hint">左边点一条问题，这里显示它的处理说明与时间线。</p>
        )}
      </section>
    </div>
  );
}

function Facets({
  title,
  facets,
  active,
  onPick,
}: {
  title: string;
  facets: Board["status_counts"];
  active: string;
  onPick: (key: string) => void;
}) {
  if (facets.length === 0) return null;
  return (
    <div className="fb-facets">
      <span className="fb-facets-title">{title}</span>
      <button className="facet" data-on={active === "" ? "true" : "false"} onClick={() => onPick("")}>
        全部 <i>{facets.reduce((sum, item) => sum + item.count, 0)}</i>
      </button>
      {facets.map((item) => (
        <button
          className="facet"
          key={item.key}
          data-on={active === item.key ? "true" : "false"}
          onClick={() => onPick(item.key)}
        >
          {item.key} <i>{item.count}</i>
        </button>
      ))}
    </div>
  );
}

function tone(status: string): "ok" | "busy" | "err" | undefined {
  if (status === "已修复") return "ok";
  if (status === "排查中" || status === "待补充") return "busy";
  if (status === "暂不处理") return "err";
  return undefined;
}

function SubmitTab({
  meta,
  related,
  onClearRelated,
  onNotice,
  onGoMine,
}: {
  meta: FeedbackMeta | null;
  related: Issue | null;
  onClearRelated: () => void;
  onNotice: (text: string, tone?: "ok" | "err") => void;
  onGoMine: () => void;
}) {
  const [form, setForm] = useState<Submission>({ ...EMPTY, occurred: nowMinute() });
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<SubmitOutcome | null>(null);
  const [error, setError] = useState("");

  const set = <K extends keyof Submission>(key: K, value: string) =>
    setForm((prev) => ({ ...prev, [key]: value }));

  const send = async () => {
    setBusy(true);
    setError("");
    try {
      const outcome = await commands.feedbackSubmit({
        ...form,
        related_issue_id: related?.id ?? "",
      });
      setResult(outcome);
      setForm({ ...EMPTY, occurred: nowMinute() });
      setConsent(false);
      onClearRelated();
      onNotice(`已提交，编号 ${outcome.id}`, "ok");
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  if (result) {
    return (
      <section className="card fb-form">
        <h2>已经收到了</h2>
        <div className="issue-meta">
          <span>编号 {result.id}</span>
          <span className="chip" data-tone={result.duplicate ? "idle" : "ok"}>
            {result.duplicate ? "已并入已有问题" : "新问题"}
          </span>
          {result.issue ? <span className="chip">{result.issue.status}</span> : null}
        </div>
        <p className="hint">
          查询凭证只写进本机私有目录，界面上不显示。随时回到「我的反馈」用这个编号看进展。
        </p>
        {result.issue?.public_note ? <p className="issue-note">{result.issue.public_note}</p> : null}
        <div className="row">
          <button className="btn" data-tone="accent" onClick={onGoMine}>
            看我的反馈
          </button>
          <button className="btn" onClick={() => setResult(null)}>
            再提一条
          </button>
        </div>
      </section>
    );
  }

  return (
    <section className="card fb-form">
      <h2>提交问题</h2>
      {related ? (
        <div className="fb-related">
          <span>挂在问题 {related.id} 下：{related.title}</span>
          <button className="btn" data-tone="ghost" onClick={onClearRelated}>
            不挂了
          </button>
        </div>
      ) : null}

      <div className="field">
        <label>标题（4–100 字）</label>
        <input
          type="text"
          value={form.title}
          placeholder="一句话说清是什么问题"
          onChange={(e) => set("title", e.target.value)}
        />
      </div>

      <div className="fb-grid">
        <div className="field">
          <label>角色名（选填，不写就按「未获取角色」）</label>
          <input
            type="text"
            value={form.character}
            placeholder="游戏里的角色名"
            onChange={(e) => set("character", e.target.value)}
          />
        </div>
        <div className="field">
          <label>发生时间</label>
          <input type="text" value={form.occurred} onChange={(e) => set("occurred", e.target.value)} />
        </div>
      </div>

      <div className="fb-grid">
        <div className="field">
          <label>问题类型</label>
          <select value={form.category} onChange={(e) => set("category", e.target.value)}>
            {(meta?.categories ?? [form.category]).map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
        </div>
        <div className="field">
          <label>影响程度</label>
          <select value={form.severity} onChange={(e) => set("severity", e.target.value)}>
            {(meta?.severities ?? [form.severity]).map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
        </div>
        <div className="field">
          <label>出现频率</label>
          <select value={form.frequency} onChange={(e) => set("frequency", e.target.value)}>
            {(meta?.frequencies ?? [form.frequency]).map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="field">
        <label>你做了什么（8–4000 字）</label>
        <textarea
          rows={4}
          value={form.steps}
          placeholder="从哪一步开始，按了什么"
          onChange={(e) => set("steps", e.target.value)}
        />
      </div>
      <div className="field">
        <label>期望看到什么（2–2000 字）</label>
        <textarea
          rows={2}
          value={form.expected}
          onChange={(e) => set("expected", e.target.value)}
        />
      </div>
      <div className="field">
        <label>实际发生了什么（2–4000 字）</label>
        <textarea
          rows={3}
          value={form.actual}
          placeholder="报错原文、卡在哪、界面什么样"
          onChange={(e) => set("actual", e.target.value)}
        />
      </div>

      <div className="fb-grid">
        <div className="field">
          <label>在哪一步（选填）</label>
          <input
            type="text"
            value={form.location}
            placeholder="城镇 / 副本名 / 菜单页"
            onChange={(e) => set("location", e.target.value)}
          />
        </div>
        <div className="field">
          <label>怎么联系你（选填）</label>
          <input
            type="text"
            value={form.contact}
            placeholder="QQ / 微信 / 邮箱"
            onChange={(e) => set("contact", e.target.value)}
          />
        </div>
      </div>

      <p className="hint">
        客户端版本与环境由启动器自己填：{meta?.client_version ?? "…"} ·{" "}
        {meta?.environment ?? "…"}。这两项和上面的正文一起上传，用于排查。
      </p>

      <button className="fb-consent" onClick={() => setConsent(!consent)}>
        <span className="fb-box" data-on={consent ? "true" : "false"} />
        <span>我确认以上内容和所附信息里没有账号、密码、验证码这类敏感东西。</span>
      </button>

      {error ? <p className="fb-error">{error}</p> : null}

      <div className="row">
        <span className="spacer" />
        <button
          className="btn"
          data-tone="accent"
          disabled={busy || !consent}
          onClick={() => void send()}
        >
          {busy ? "正在提交…" : "提交"}
        </button>
      </div>
      <p className="hint">
        一小时里最多提 6 条，这是站上的限流；被挡了等几分钟再来，内容会留在这一页里。
      </p>
    </section>
  );
}

function MineTab({ onGoSubmit }: { onGoSubmit: () => void }) {
  const [list, setList] = useState<Mine[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      setList(await commands.feedbackMine());
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <section className="card fb-form">
      <div className="row">
        <h2>我的反馈</h2>
        <span className="spacer" />
        <button className="btn" disabled={busy} onClick={() => void load()}>
          {busy ? "正在查…" : "刷新"}
        </button>
      </div>
      {error ? <p className="fb-error">{error}</p> : null}
      {list === null && !error ? <p className="hint">正在用本机的编号换最新状态…</p> : null}
      {list && list.length === 0 ? (
        <p className="hint">
          本机还没有站上认得的反馈。提一条，或者开着自动上报让启动器替你记一笔。
        </p>
      ) : null}
      {list && list.length > 0 ? (
        <div className="report-list">
          {list.map((item) => (
            <div className="report-item" key={item.id}>
              <div className="report-head">
                <b>{item.title}</b>
                <span className="chip" data-tone={tone(item.issue?.status ?? item.status)}>
                  {item.issue?.status ?? item.status}
                </span>
              </div>
              <div className="report-meta">
                <span>编号 {item.id}</span>
                {item.issue ? <span>问题 {item.issue.id}</span> : null}
                {item.issue ? <span>{item.issue.report_count} 份反馈</span> : null}
                {item.issue?.fix_version ? <span>修复版本 {item.issue.fix_version}</span> : null}
                <span>{dateText(item.created)}</span>
              </div>
              {item.issue?.public_note ? (
                <p className="hint">{item.issue.public_note}</p>
              ) : (
                <p className="hint">
                  {item.issue ? "管理员还没有公开处理说明。" : "这条还没归到某条问题上。"}
                </p>
              )}
            </div>
          ))}
        </div>
      ) : null}
      <div className="row">
        <span className="spacer" />
        <button className="btn" onClick={onGoSubmit}>
          去提交问题
        </button>
      </div>
      <p className="hint">
        凭证只存在本机私有目录：换电脑、删掉启动器数据后，这一页就找不回旧记录了。
      </p>
    </section>
  );
}
