"use client";

// 用量时序柱状图:基于 shadcn chart + recharts。
import { Bar, BarChart, CartesianGrid, XAxis } from "recharts";
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";

const chartConfig = {
  requests: {
    label: "Requests",
    color: "#d79921",
  },
} satisfies ChartConfig;

export function UsageChart({ points }: { points: Array<{ bucket_start: string; value: number }> }) {
  const data = points.map((p) => ({
    bucket: p.bucket_start.slice(11, 16) || p.bucket_start,
    requests: p.value,
  }));

  return (
    <ChartContainer config={chartConfig} className="h-48 w-full">
      <BarChart data={data} margin={{ top: 8, right: 8, left: 8, bottom: 0 }}>
        <CartesianGrid vertical={false} strokeDasharray="3 3" />
        <XAxis
          dataKey="bucket"
          tickLine={false}
          axisLine={false}
          tickMargin={8}
          minTickGap={24}
          fontSize={10}
        />
        <ChartTooltip
          cursor={{ fill: "rgba(124,111,100,0.12)" }}
          content={<ChartTooltipContent />}
        />
        <Bar dataKey="requests" fill="var(--color-requests)" radius={[3, 3, 0, 0]} />
      </BarChart>
    </ChartContainer>
  );
}
