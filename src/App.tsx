import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DashboardData, RateLimit, TokenValues, UsageBucket } from "./types";

const SERIES: { key: keyof TokenValues; label: string; color: string }[] = [
  { key: "input", label: "Input", color: "#61afef" },
  { key: "output", label: "Output", color: "#e06c75" },
  { key: "cacheRead", label: "Cache read", color: "#98c379" },
  { key: "cacheWrite", label: "Cache write", color: "#e5c07b" },
  { key: "reasoning", label: "Reasoning", color: "#c678dd" },
];

const number = new Intl.NumberFormat("ja-JP", { notation: "compact", maximumFractionDigits: 1 });
const dateTime = new Intl.DateTimeFormat("ja-JP", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

function remainingTime(resetsAt: number, period: "five-hour" | "weekly", now: number) {
  const minutes = Math.max(0, Math.ceil((resetsAt * 1000 - now) / 60_000));
  if (period === "weekly" && minutes >= 24 * 60) {
    return `残り${Math.floor(minutes / (24 * 60))}日`;
  }
  return `残り${Math.floor(minutes / 60)}時間${minutes % 60}分`;
}

function Gauge({ title, limit, period, now }: {
  title: string;
  limit: RateLimit | null;
  period: "five-hour" | "weekly";
  now: number;
}) {
  const value = Math.min(100, Math.max(0, limit?.usedPercent ?? 0));
  const color = value >= 90 ? "#e06c75" : value >= 70 ? "#e5c07b" : "#61afef";
  const isStale = !!limit?.resetsAt && limit.resetsAt * 1000 <= now;
  return (
    <article className="gauge-card">
      <div className="gauge">
        <svg viewBox="0 0 82 82" aria-hidden="true">
          <circle className="gauge-track" cx="41" cy="41" r="34" pathLength="100" />
          <circle
            className="gauge-value"
            cx="41"
            cy="41"
            r="34"
            pathLength="100"
            stroke={color}
            strokeDasharray={`${value} 100`}
          />
        </svg>
        <div className="gauge-center">
          <strong>{isStale ? "—" : limit ? `${Math.round(value)}%` : "—"}</strong>
          <span>{isStale ? "未更新" : "used"}</span>
        </div>
      </div>
      <div>
        <h3>{title}</h3>
        <p>{isStale
          ? `リセット時刻（${dateTime.format(new Date(limit!.resetsAt! * 1000))}）を過ぎています（未使用のため未更新）`
          : limit?.resetsAt
          ? `リセット ${dateTime.format(new Date(limit.resetsAt * 1000))}（${remainingTime(limit.resetsAt, period, now)}）`
          : limit ? "リセット時刻は未取得" : "データ待ち"}</p>
      </div>
    </article>
  );
}

function StackedChart({ buckets }: { buckets: UsageBucket[] }) {
  const width = 1000;
  const height = 300;
  const top = 18;
  const bottom = 48;
  const left = 58;
  const right = 18;
  const plotHeight = height - top - bottom;
  const plotWidth = width - left - right;
  const totals = buckets.map((bucket) => SERIES.reduce((sum, series) => sum + bucket[series.key], 0));
  const max = Math.max(1, ...totals);
  const barSpace = buckets.length ? plotWidth / buckets.length : plotWidth;
  const barWidth = Math.max(4, Math.min(48, barSpace * 0.66));

  return (
    <div className="chart-wrap">
      <svg className="chart" viewBox={`0 0 ${width} ${height}`} role="img" aria-label="直近5時間の10分ごとのトークン使用量">
        {[0, 0.5, 1].map((ratio) => {
          const y = top + plotHeight * (1 - ratio);
          return (
            <g key={ratio}>
              <line x1={left} x2={width - right} y1={y} y2={y} className="grid-line" />
              <text x={left - 10} y={y + 4} textAnchor="end" className="axis-text">{number.format(max * ratio)}</text>
            </g>
          );
        })}
        {buckets.map((bucket, index) => {
          const x = left + index * barSpace + (barSpace - barWidth) / 2;
          let usedHeight = 0;
          return (
            <g key={bucket.start}>
              <title>{`${dateTime.format(new Date(bucket.start))}–${dateTime.format(new Date(bucket.end))}\n${number.format(totals[index])} tokens`}</title>
              {SERIES.map((series) => {
                const segmentHeight = (bucket[series.key] / max) * plotHeight;
                const y = top + plotHeight - usedHeight - segmentHeight;
                usedHeight += segmentHeight;
                return <rect key={series.key} x={x} y={y} width={barWidth} height={segmentHeight} fill={series.color} rx="2" />;
              })}
              {(index % Math.max(1, Math.ceil(buckets.length / 6)) === 0 || index === buckets.length - 1) && (
                <text x={x + barWidth / 2} y={height - 18} textAnchor="middle" className="axis-text">
                  {new Intl.DateTimeFormat("ja-JP", { month: "numeric", day: "numeric", hour: "numeric" }).format(new Date(bucket.start))}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      <div className="legend">
        {SERIES.map((series) => <span key={series.key}><i style={{ background: series.color }} />{series.label}</span>)}
      </div>
    </div>
  );
}

function App() {
  const [data, setData] = useState<DashboardData | null>(null);
  const [selected, setSelected] = useState<"codex" | "claude">("codex");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const [loggingIn, setLoggingIn] = useState(false);
  const refreshing = useRef(false);

  const refresh = useCallback(async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    setLoading(true);
    setError(null);
    try {
      setData(await invoke<DashboardData>("get_dashboard_data", { bucketCount: 30 }));
    } catch (reason) {
      setError(String(reason));
    } finally {
      refreshing.current = false;
      setLoading(false);
    }
  }, []);

  const loginClaude = useCallback(async () => {
    if (loggingIn) return;
    setLoggingIn(true);
    setError(null);
    try {
      await invoke("login_claude");
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoggingIn(false);
    }
  }, [loggingIn, refresh]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 10 * 60 * 1000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60 * 1000);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <main>
      {error && <div className="notice error">{error}</div>}
      {data?.warnings.map((warning) => <div className="notice" key={warning}>{warning}</div>)}
      <section>
        <div className="section-heading">
          <h2>現在の上限使用率</h2>
          <div className="header-actions">
            {data?.claudeLoginRequired && (
              <button className="refresh-button" onClick={() => void loginClaude()} disabled={loggingIn}>
                {loggingIn ? "ログイン中…" : "Claudeログイン"}
              </button>
            )}
            <button className="refresh-button" onClick={() => void refresh()} disabled={loading}>
              {loading ? "更新中…" : "更新"}
            </button>
          </div>
        </div>
        <div className="gauges">
          <Gauge title="Codex · 5時間" limit={data?.codex.fiveHour ?? null} period="five-hour" now={now} />
          <Gauge title="Codex · 週次" limit={data?.codex.weekly ?? null} period="weekly" now={now} />
          <Gauge title="Claude · 5時間" limit={data?.claude.fiveHour ?? null} period="five-hour" now={now} />
          <Gauge title="Claude · 週次" limit={data?.claude.weekly ?? null} period="weekly" now={now} />
        </div>
      </section>
      <section className="chart-section">
        <div className="section-heading">
          <h2>直近5時間のトークン使用量</h2>
          <div className="tabs">
            <button className={selected === "codex" ? "active" : ""} onClick={() => setSelected("codex")}>Codex</button>
            <button className={selected === "claude" ? "active" : ""} onClick={() => setSelected("claude")}>Claude</button>
          </div>
        </div>
        <StackedChart buckets={data?.[selected].buckets ?? []} />
        <p className="footnote">直近5時間のローカルJSONLを、10分単位で集計しています。</p>
      </section>
    </main>
  );
}

export default App;
