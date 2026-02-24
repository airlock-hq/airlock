# Airlock Design System

## Vision

Airlock should feel:

- Futuristic
- Clean
- Autonomous
- Intelligent
- Infrastructure-grade

It should **not** feel:

- SaaS-blue
- Marketing-heavy
- Visually noisy
- Cyberpunk
- Over-designed

The aesthetic target:

> Apple × NASA × Infrastructure Software

White-dominant.
Minimal.
Precise.
Orbital violet used as a **signal**, not decoration.

---

# Operating Model

We are adopting a **code-first design discipline**.

No required Figma layer.

Design rigor is enforced through:

- Strict semantic tokens
- Centralized component variants
- Lint + CI guardrails
- A dedicated `/design-system` page
- Zero tolerance for visual drift

Design entropy is treated as technical debt.

---

# Core Principles

## 1. Violet Is a Signal, Not a Theme

Orbital violet appears only when:

- Something is active
- Something is running
- Something is focused
- Something needs attention

It should not:

- Fill large surfaces
- Be used as decoration
- Appear on every primary button
- Dominate the interface

Color = system activity.

Absence of color = calm autonomy.

---

## 2. Monochrome by Default

Primary UI consists of:

- White surfaces
- Soft cool grays
- Graphite text

Color only appears when the system is doing something.

This reinforces the “autonomous infrastructure” feeling.

---

## 3. Design Discipline Is Enforced in Code

All visual styling must:

- Use semantic tokens
- Avoid raw hex values
- Avoid Tailwind palette colors
- Avoid arbitrary color utilities
- Avoid inline color styles

Every component variant must be centralized.

No one-off styling.

---

# Initial Design Tokens

All UI must reference semantic tokens.

## Light Theme Tokens

```css
:root {
  /* ===== Surfaces (cooled) ===== */
  --background: 0 0% 100%;
  --surface: 220 25% 98%;
  --surface-elevated: 220 25% 96%;

  /* ===== Text ===== */
  --foreground: 220 30% 10%;
  --foreground-muted: 220 15% 45%;

  /* ===== Borders ===== */
  --border: 220 20% 88%;
  --border-subtle: 220 20% 92%;

  /* ===== Orbital Violet (Signal) ===== */
  --signal: 250 55% 55%;
  --signal-subtle: 250 60% 96%;
  --signal-glow: 250 70% 65%;

  /* ===== Signal State Variants ===== */
  --signal-active: 250 55% 55%;
  --signal-focus: 250 55% 55%;
  --signal-attention: 250 60% 50%;

  /* ===== Status Colors ===== */
  --success: 145 60% 40%;
  --warning: 38 90% 50%;
  --danger: 0 70% 50%;

  /* ===== Atmosphere (Landing / Decorative) ===== */
  --atmosphere-orbit-line: 220 20% 90%;
  --atmosphere-glow-opacity: 0.08;
  --atmosphere-particle-opacity: 0.12;

  /* ===== Focus Ring ===== */
  --ring: 250 55% 55%;
}
```

Approximate hex references:

- `--signal` ≈ `#7B7AAE`
- `--signal-glow` ≈ `#8F8EDB` (use at low opacity)

---

# Typography System

Typography is part of the design system and should be represented as:

- Font choices
- Type scale (sizes + line heights)
- Semantic roles (what each style is used for)
- UI text rules (tracking, casing, numeric alignment)

## Font Stack

**Primary UI font** (clean, modern):

- Inter (recommended default)

**System/telemetry font** (optional but very on-theme):

- JetBrains Mono (installed as `@fontsource-variable/jetbrains-mono`)

Example CSS variables:

```css
:root {
  --font-sans:
    'Inter Variable', Inter, ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, Arial, 'Apple Color Emoji',
    'Segoe UI Emoji';
  --font-mono:
    'JetBrains Mono Variable', 'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas,
    'Liberation Mono', 'Courier New', monospace;
}
```

## Type Scale (Initial)

Use a small, disciplined scale to avoid drift.

| Role            | Size / Line Height | Notes                                      |
| --------------- | ------------------ | ------------------------------------------ |
| Display (Hero)  | 48px / 56px        | tracking: -0.02em                          |
| H1 (Page)       | 32px / 40px        |                                            |
| H2 (Section)    | 24px / 32px        |                                            |
| Body            | 16px / 24px        |                                            |
| Small           | 14px / 20px        |                                            |
| Micro/Telemetry | 12px / 16px        | uppercase, tracking: 0.08em, mono optional |

## Semantic Roles

- **Hero Title:** Display size, sparse, confident
- **Page Title:** H1
- **Section Title:** H2
- **Body:** default content
- **UI Labels:** Small
- **Telemetry Tags:** Micro, uppercase, tracking-wide, muted, mono

## Numeric Display Rule

Numeric columns must use tabular numbers:

```css
font-variant-numeric: tabular-nums;
```

This reinforces the infrastructure feel and prevents layout shift in data tables and workflow UIs.

## Telemetry Style Rules

Telemetry should read like an engineered console:

- `font-mono` recommended
- uppercase
- `tracking-wide`
- muted tone
- minimal punctuation

Example:

```
SYSTEM STATUS — VALIDATING
MODE — LOCAL_EXECUTION
SYNC — ACTIVE
```

---

# Component Guidelines

## Buttons

- Default button is neutral (surface + border + foreground)
- Violet appears on hover/focus/active via ring/glow
- "Signal" variant: neutral idle (border + graphite text), violet glow on hover
  - Idle = calm, Hover = system activation
  - Never a filled violet button — violet is earned by interaction
- Use signal variant sparingly (primary CTAs only)

## Workflow List Rows (Core UI)

- Prefer flat rows over heavy cards
- Use hairline separators (`border-subtle`)
- Running state: violet dot + subtle glow + optional left rail
- Success/failure: tiny status dot (no big colored fills)

## Surfaces

- Use minimal shadows (or none)
- Favor borders over elevation
- Keep spacing generous; avoid UI density creep

---

# Motion Guidelines

- 200–300ms transitions
- Opacity + slight translate only (2–4px)
- Subtle "alive" effects:
  - slow pulse on running dot
  - gentle fade of telemetry status
- Avoid dramatic easing, bouncing, scaling
- **Motion should never imply excitement. Motion implies system state change only.**

The interface should feel calm and autonomous.

---

# Guardrails (Mandatory)

- No raw hex in components
- No Tailwind palette colors (`text-violet-500`, `bg-blue-600`, etc.)
- No arbitrary color utilities (`text-[#...]`, `bg-[...]`)
- No custom spacing scale values — only predefined spacing tokens allowed
- Prefer no arbitrary spacing (`mt-[13px]`) except rare escapes
- No inline color styles (`style={{ color: ... }}`)
- All variants centralized (cva/shadcn variants)
- All visual changes reviewed via `/design-system`
- Lint/CI should fail on violations

---

# System Showcase Requirement (`/design-system`)

Maintain a route that displays:

- Buttons (all variants + states)
- Badges
- Tabs
- Inputs
- Dialogs
- Workflow row states (queued/running/success/failure)
- Telemetry labels
- Surface patterns (page, panel, elevated)
- Empty + error states

All design changes are validated there first.

---

# Long-Term Direction

Airlock should feel like:

- A control system
- Infrastructure
- Something quietly running beneath your stack

Violet remains restrained and intentional.

Calmness signals confidence.
Discipline signals intelligence.
