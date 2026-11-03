import { useEffect, useRef } from "react";
import * as echarts from "echarts/core";
import { LineChart } from "echarts/charts";
import { AriaComponent, GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
import { SVGRenderer } from "echarts/renderers";
import { useTheme } from "../theme";
import { shortPod, valueText } from "../telemetry";
import type { TelemetryHistory } from "../telemetry";

echarts.use([LineChart, GridComponent, LegendComponent, TooltipComponent, AriaComponent, SVGRenderer]);

export function TelemetryChart({ data, title }: { data: TelemetryHistory; title: string }) {
  const host = useRef<HTMLDivElement>(null);
  const { theme } = useTheme();
  useEffect(() => {
    if (!host.current) return;
    const chart = echarts.init(host.current, undefined, { renderer: "svg" });
    const css = getComputedStyle(document.documentElement);
    const color = (name: string) => css.getPropertyValue(name).trim();
    chart.setOption({
      animation: false,
      color: ["#2bb99c", "#62a5ef", "#e9a654", "#de7183", "#a4c36e", "#b7a2dd", "#6fc8ce", "#c39d8f"],
      aria: { enabled: true, description: title },
      textStyle: { fontFamily: "system-ui, sans-serif", color: color("--text-soft") },
      grid: { left: 14, right: 22, top: 20, bottom: data.series.length > 1 ? 58 : 20, containLabel: true },
      tooltip: { trigger: "axis", renderMode: "richText", backgroundColor: color("--panel"),
        textStyle: { color: color("--text") }, borderColor: color("--border") },
      legend: { show: data.series.length > 1, type: "scroll", bottom: 0,
        textStyle: { color: color("--text-soft"), fontSize: 11 }, pageTextStyle: { color: color("--text-soft") } },
      xAxis: { type: "time", axisLine: { lineStyle: { color: color("--border") } },
        axisLabel: { hideOverlap: true, color: color("--muted"), fontSize: 11 }, splitLine: { show: false } },
      yAxis: { type: "value", min: 0, axisLabel: { color: color("--muted"), fontSize: 11,
        formatter: (value: number) => valueText(value, data.unit) },
        splitLine: { lineStyle: { color: color("--border"), type: "dashed" } } },
      series: data.series.map((series, index) => ({
        type: "line", name: shortPod(series.labels.pod || series.labels.role || series.labels.node || `Series ${index + 1}`) + (series.labels.state ? ` ${series.labels.state}` : ""),
        data: series.values.map(([time, value]) => [time * 1000, value]),
        showSymbol: false, connectNulls: false, lineStyle: { width: 2, type: series.labels.state === "limit" ? "dashed" : "solid" },
        emphasis: { focus: "series" },
      })),
    });
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => chart.resize());
    observer?.observe(host.current);
    const resize = () => chart.resize();
    window.addEventListener("resize", resize);
    return () => { observer?.disconnect(); window.removeEventListener("resize", resize); chart.dispose(); };
  }, [data, theme, title]);
  return <div className="telemetry-chart" ref={host} role="img" aria-label={title} />;
}
