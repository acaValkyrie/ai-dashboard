import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DashboardData, RateLimit, TokenValues, UsageBucket } from "./types";

const SERIES: { key: keyof TokenValues; label: string; color: string }[] = [
  { key: "input", label: "Input", color: "#60a5fa" },
  { key: "output", label: "Output", color: "#f472b6" },
  { key: "cacheRead", label: "Cache read", color: "#34d399" },
  { key: "cacheWrite", label: "Cache write", color: "#fbbf24" },
  { key: "reasoning", label: "Reasoning", color: "#a78bfa" },
];

const number = new Intl.NumberFormat("ja-JP", { notation: "compact", maximumFractionDigits: 1 });
const dateTime = new Intl.DateTimeFormat("ja-JP", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

function Gauge({ title, limit }: { title: string; limit: RateLimit | null }) {
  const value = Math.min(100, Math.max(0, limit?.usedPercent ?? 0));
  const color = value >= 90 ? "#fb7185" : value >= 70 ? "#fbbf24" : "#38bdf8";
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
          <strong>{limit ? `${Math.round(value)}%` : "—"}</strong>
          <span>used</span>
        </div>
      </div>
      <div>
        <h3>{title}</h3>
        <p>{limit?.resetsAt
          ? `リセット ${dateTime.format(new Date(limit.resetsAt * 1000))}`
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

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 10 * 60 * 1000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  return (
    <main>
      <header>
        <div><p className="eyebrow">LOCAL USAGE MONITOR</p><h1>AI Usage</h1></div>
        <button onClick={() => void refresh()} disabled={loading}>{loading ? "更新中…" : "更新"}</button>
      </header>
      {error && <div className="notice error">{error}</div>}
      {data?.warnings.map((warning) => <div className="notice" key={warning}>{warning}</div>)}
      <section>
        <div className="section-heading"><div><p className="eyebrow">CURRENT LIMITS</p><h2>現在の上限使用率</h2></div></div>
        <div className="gauges">
          <Gauge title="Codex · 5時間" limit={data?.codex.fiveHour ?? null} />
          <Gauge title="Codex · 週次" limit={data?.codex.weekly ?? null} />
          <Gauge title="Claude · 5時間" limit={data?.claude.fiveHour ?? null} />
          <Gauge title="Claude · 週次" limit={data?.claude.weekly ?? null} />
        </div>
      </section>
      <section className="chart-section">
        <div className="section-heading">
          <div><p className="eyebrow">LAST 5 HOURS</p><h2>直近5時間のトークン使用量</h2></div>
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
