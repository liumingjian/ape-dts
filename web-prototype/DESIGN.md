# ape-dts Console Design System

## 1. Atmosphere & Identity

ape-dts Console is a quiet enterprise operations console: dense, legible, and incident-aware. The signature is a deep-teal control surface with restrained severity accents, compact tables, and responsive navigation that keeps operators focused on the current risk.

## 2. Color

### Palette

| Role | Token | Light | Dark | Usage |
|------|-------|-------|------|-------|
| Brand primary | --color-primary-700 | #0F766E | n/a | Active nav, key actions |
| Brand accent | --color-accent | #06B6D4 | n/a | Secondary data accent |
| Canvas | --color-canvas | #F8FAFC | n/a | App background |
| Surface | --color-surface | #FFFFFF | n/a | Panels, cards, topbar |
| Surface secondary | --color-surface-2 | #F1F5F9 | n/a | Hover, subtle bands |
| Ink primary | --color-ink | #0F172A | n/a | Primary text |
| Ink muted | --color-ink-muted | #475569 | n/a | Secondary text |
| Ink subtle | --color-ink-subtle | #64748B | n/a | Captions, hints |
| Border | --color-border | #E2E8F0 | n/a | Panel and table dividers |
| Success | --color-success | #10B981 | n/a | Healthy/running state |
| Warning | --color-warning | #F59E0B | n/a | Major/warning state |
| Danger | --color-danger | #EF4444 | n/a | Critical/failure state |
| Info | --color-info | #0EA5E9 | n/a | Minor/info state |

### Rules

- Brand teal is for navigation, primary actions, focus, and live operations only.
- Severity colors carry operational meaning and must not be used decoratively.
- Charts use teal for throughput, amber for latency, red/amber/blue/slate for alert severity.

## 3. Typography

### Scale

| Level | Size | Weight | Line Height | Tracking | Usage |
|-------|------|--------|-------------|----------|-------|
| H1 | 22px | 600 | 1.3 | 0 | Page title |
| H2 | 16px | 600 | 1.4 | 0 | Section title |
| H3 | 15px | 600 | 1.4 | 0 | Panel title |
| Body | 14px | 400 | 1.5 | 0 | Default UI |
| Body/sm | 13px | 400 | 1.45 | 0 | Table cells |
| Caption | 12px | 500 | 1.4 | 0 | Metadata |
| Mono | 12px | 400 | 1.4 | 0 | IDs, IPs, metrics |

### Font Stack

- Primary: `HarmonyOS Sans SC`, `PingFang SC`, `Source Han Sans CN`, `Microsoft YaHei`, Inter, system-ui, sans-serif.
- Mono: `JetBrains Mono`, `SF Mono`, Consolas, Menlo, monospace.

### Rules

- Data-heavy numbers use tabular figures.
- Page headers wrap naturally, but topbar breadcrumbs and account controls must stay horizontal.

## 4. Spacing & Layout

### Base Unit

All spacing derives from 4px tokens already declared in `src/styles/tokens.css`.

| Token | Value | Usage |
|-------|-------|-------|
| --space-1 | 4px | Tight icon gaps |
| --space-2 | 8px | Compact control gaps |
| --space-3 | 12px | Filter and toolbar gaps |
| --space-4 | 16px | Dense panel padding |
| --space-5 | 20px | Page header/body rhythm |
| --space-6 | 24px | Desktop page gutters |
| --space-8 | 32px | Major groups |

### Grid

- Desktop shell: 224px sidebar, 56px topbar.
- Collapsed shell: 64px sidebar.
- Mobile shell under 768px: header plus off-canvas sidebar overlay; content uses 16px gutters.
- Tables may scroll horizontally inside their panels on small screens; the app shell itself must not create horizontal scroll.

## 5. Components

### App Shell

- **Structure**: sidebar, topbar, content region, license banner, routed page.
- **Variants**: desktop expanded, desktop collapsed, mobile overlay.
- **States**: sidebar open/closed, active navigation, hover/focus on controls.
- **Accessibility**: menu trigger has an accessible label; overlay can be dismissed.
- **Motion**: sidebar uses transform and opacity only on mobile.

### Page Header

- **Structure**: title/subtitle left, action group right.
- **Variants**: actionless, dense action group.
- **States**: actions wrap below title on narrow widths.
- **Accessibility**: page title remains the first visible heading.

### Metric/KPI Card

- **Structure**: label/icon, value/unit, delta, sparkline.
- **Variants**: default, accent, success, warning, danger.
- **States**: hover, focus-visible for clickable cards.
- **Motion**: subtle translate and shadow only.

### Data Panel

- **Structure**: toolbar, filters, table/chart body, footer pagination.
- **Variants**: dashboard chart, task table, alert table, metric rules.
- **States**: loading, empty, filtered, selected rows.
- **Accessibility**: row actions are real buttons; table content keeps readable contrast.

### Severity Summary

- **Structure**: level label, count, optional total card.
- **Variants**: critical, major, minor, info, total.
- **States**: default, hover, active filter.
- **Accessibility**: Critical and major have stronger visual hierarchy than minor/info.

## 6. Motion & Interaction

| Type | Duration | Easing | Usage |
|------|----------|--------|-------|
| Micro | 120ms | var(--ease-soft) | Button press, hover |
| Standard | 180ms | var(--ease-soft) | Sidebar collapse, cards |
| Emphasis | 260ms | var(--ease-soft) | Mobile sidebar overlay |

Rules:

- Animate `transform`, `opacity`, `box-shadow`, and color transitions only.
- Every clickable icon button has hover and focus-visible states.
- Respect `prefers-reduced-motion` by disabling non-essential transitions.

## 7. Depth & Surface

### Strategy

Mixed: quiet borders for data surfaces plus subtle shadows only for elevated panels and hover. Cards use radius 6-8px; controls stay compact.

| Level | Token | Usage |
|-------|-------|-------|
| Card | --shadow-card | Resting panels |
| Elevated | --shadow-elevated | Hovered cards, dropdown-like emphasis |
| Drop | --shadow-drop | Mobile sidebar overlay |

Rules:

- Avoid nested cards unless the inner surface is a repeated item or real tool.
- Dashboard density should prioritize scan speed over decorative whitespace.
