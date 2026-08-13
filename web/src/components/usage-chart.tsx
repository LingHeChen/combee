// 用量时序柱状图:纯 SVG,无第三方图表依赖。
// points 为 /v1/usage/timeseries 返回的 { bucket_start, value } 序列。
export function UsageChart({ points }: { points: Array<{ bucket_start: string; value: number }> }) {
  const max = Math.max(...points.map((p) => p.value), 1);
  const w = 600;
  const h = 200;
  const barW = points.length > 0 ? w / points.length : 0;

  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      className="w-full h-48"
      preserveAspectRatio="none"
      role="img"
      aria-label="usage chart"
      data-testid="usage-chart"
    >
      {points.map((p, i) => {
        const bh = Math.max((p.value / max) * (h - 24), p.value > 0 ? 2 : 0);
        const x = i * barW + 1;
        const y = h - bh - 12;
        return (
          <g key={p.bucket_start}>
            <rect
              x={x}
              y={y}
              width={Math.max(barW - 2, 1)}
              height={bh}
              rx={2}
              className="fill-[#7c6f64]/60 hover:fill-[#d79921]"
            />
            <title>{`${p.bucket_start}: ${p.value}`}</title>
          </g>
        );
      })}
    </svg>
  );
}
