/**
 * Register ECharts components once per module load. Import this file from any
 * place that renders a chart — subsequent imports are no-ops thanks to ESM.
 */
import { use } from 'echarts/core';
import { CanvasRenderer } from 'echarts/renderers';
import {
  LineChart, BarChart, PieChart, GaugeChart, ScatterChart,
} from 'echarts/charts';
import {
  GridComponent, TooltipComponent, LegendComponent, DataZoomComponent,
  TitleComponent, MarkLineComponent, MarkPointComponent, VisualMapComponent,
  DatasetComponent, TransformComponent,
} from 'echarts/components';

use([
  CanvasRenderer,
  LineChart, BarChart, PieChart, GaugeChart, ScatterChart,
  GridComponent, TooltipComponent, LegendComponent, DataZoomComponent,
  TitleComponent, MarkLineComponent, MarkPointComponent, VisualMapComponent,
  DatasetComponent, TransformComponent,
]);

export const BRAND_PALETTE = [
  '#0F766E', // primary teal
  '#06B6D4', // cyan accent
  '#F59E0B', // warning amber
  '#EF4444', // danger red
  '#6366F1', // indigo
  '#10B981', // emerald
  '#8B5CF6', // violet
  '#EC4899', // pink
];

export const AXIS_BASE = {
  axisLine: { lineStyle: { color: '#E2E8F0' } },
  axisLabel: { color: '#64748B', fontSize: 11 },
  splitLine: { lineStyle: { color: '#F1F5F9' } },
};
