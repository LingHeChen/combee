---
name: High-Density Engineering Core
colors:
  surface: '#121414'
  surface-dim: '#121414'
  surface-bright: '#383939'
  surface-container-lowest: '#0c0f0e'
  surface-container-low: '#1a1c1c'
  surface-container: '#1e2020'
  surface-container-high: '#282a2a'
  surface-container-highest: '#333535'
  on-surface: '#e2e2e2'
  on-surface-variant: '#c4c7c7'
  inverse-surface: '#e2e2e2'
  inverse-on-surface: '#2f3130'
  outline: '#8e9192'
  outline-variant: '#444748'
  surface-tint: '#c9c6c5'
  primary: '#c9c6c5'
  on-primary: '#313030'
  primary-container: '#0a0a0a'
  on-primary-container: '#7b7979'
  inverse-primary: '#5f5e5e'
  secondary: '#bdc7d9'
  on-secondary: '#27313f'
  secondary-container: '#404a59'
  on-secondary-container: '#afb9cb'
  tertiary: '#ffb95f'
  on-tertiary: '#472a00'
  tertiary-container: '#130800'
  on-tertiary-container: '#aa6c00'
  error: '#ffb4ab'
  on-error: '#690005'
  error-container: '#93000a'
  on-error-container: '#ffdad6'
  primary-fixed: '#e5e2e1'
  primary-fixed-dim: '#c9c6c5'
  on-primary-fixed: '#1c1b1b'
  on-primary-fixed-variant: '#474646'
  secondary-fixed: '#d9e3f6'
  secondary-fixed-dim: '#bdc7d9'
  on-secondary-fixed: '#121c2a'
  on-secondary-fixed-variant: '#3d4756'
  tertiary-fixed: '#ffddb8'
  tertiary-fixed-dim: '#ffb95f'
  on-tertiary-fixed: '#2a1700'
  on-tertiary-fixed-variant: '#653e00'
  background: '#121414'
  on-background: '#e2e2e2'
  surface-variant: '#333535'
typography:
  display-lg:
    fontFamily: Geist
    fontSize: 48px
    fontWeight: '700'
    lineHeight: 56px
    letterSpacing: -0.04em
  headline-lg:
    fontFamily: Geist
    fontSize: 32px
    fontWeight: '600'
    lineHeight: 40px
    letterSpacing: -0.02em
  headline-lg-mobile:
    fontFamily: Geist
    fontSize: 24px
    fontWeight: '600'
    lineHeight: 32px
    letterSpacing: -0.02em
  headline-md:
    fontFamily: Geist
    fontSize: 20px
    fontWeight: '500'
    lineHeight: 28px
    letterSpacing: -0.01em
  body-lg:
    fontFamily: Geist
    fontSize: 16px
    fontWeight: '400'
    lineHeight: 24px
    letterSpacing: 0em
  body-sm:
    fontFamily: Geist
    fontSize: 14px
    fontWeight: '400'
    lineHeight: 20px
    letterSpacing: 0em
  mono-label:
    fontFamily: JetBrains Mono
    fontSize: 13px
    fontWeight: '500'
    lineHeight: 16px
    letterSpacing: 0.02em
  mono-code:
    fontFamily: JetBrains Mono
    fontSize: 14px
    fontWeight: '400'
    lineHeight: 22px
    letterSpacing: 0em
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  unit: 4px
  gutter: 16px
  margin-mobile: 16px
  margin-desktop: 32px
  container-max: 1280px
---

## Brand & Style

This design system targets high-performance engineering teams requiring a UI that reflects reliability, speed, and precision. The brand personality is "Technical Authority"—it is silent, efficient, and sophisticated.

The style is **Modern Corporate Minimalism** with a **Technical/Developer-First** edge. It draws inspiration from high-end hardware and developer productivity tools, utilizing a "Dark Mode by Default" philosophy. Visual interest is generated through precise geometry, high-contrast typography, and purposeful light-leaks rather than decorative illustrations. The hexagonal motif is used strictly for structural data visualization or grid-based alignment, reinforcing the concept of modularity and strength.

## Colors

The palette is engineered for prolonged focus and high legibility in low-light environments.

*   **Deep Charcoal (#0A0A0A):** The foundation. Used for the primary canvas to reduce eye strain and provide a void-like depth.
*   **Warm Amber (#F59E0B):** The signal. Used sparingly for critical primary actions, active states, and success indicators. It represents the "energy" within the infrastructure.
*   **Soft Cream (#FAFAF9):** The content. A slightly desaturated white to prevent "halation" or glowing text against the dark background.
*   **Slate Grays (#1F2937, #374151):** The structure. Used for container surfaces, dividers, and secondary UI elements to create a clear information hierarchy without high-contrast borders.

## Typography

Typography is the primary driver of the UI's credibility. We use **Geist** for its mathematical precision and neutral, "engineered" aesthetic. **JetBrains Mono** is reserved for all data points, system logs, code snippets, and small labels to signal technical context.

Maintain tight letter-spacing on headlines to create a compact, "premium" feel. For body text, ensure generous line-height to maintain readability amidst high information density. All labels and metadata should be uppercase and monospaced when referring to system states or metrics.

## Layout & Spacing

This design system utilizes a **Fixed Grid** model for dashboards and a **Fluid Content** model for documentation.

*   **Grid:** A 12-column grid system is used for the main dashboard. Gutters are kept at a tight 16px to maintain high information density.
*   **Rhythm:** All spacing must be a multiple of 4px. Use 8px and 16px for internal component padding, and 32px+ for sectional separation.
*   **Hexagonal Alignment:** While not using visible hex shapes for layout, ensure elements follow a staggered "honeycomb" visual weight—meaning vertical alignment is prioritized, but secondary information can be offset to create a dynamic, technical flow.
*   **Breakpoints:**
    *   Mobile (<768px): 4-column layout, 16px margins.
    *   Tablet (768px-1024px): 8-column layout, 24px margins.
    *   Desktop (>1024px): 12-column layout, 32px margins.

## Elevation & Depth

Depth is conveyed through **Tonal Layering** and **Subtle Glows** rather than traditional shadows.

1.  **Base (Level 0):** `#0A0A0A` — The background.
2.  **Surface (Level 1):** `#111111` — Primary cards and navigation sidebars.
3.  **Overlay (Level 2):** `#1F2937` — Modals, popovers, and tooltips.

**Visual Treatments:**
*   **Borders:** All cards use a consistent `1px` border in `#1F2937`.
*   **Active State Glow:** When a card or element is focused/active, apply a very soft, diffused outer glow using the Primary Amber color at 10% opacity with a 20px blur. 
*   **Glassmorphism:** Use `backdrop-filter: blur(12px)` on navigation bars and floating headers to maintain context of the content scrolling beneath.

## Shapes

The shape language is "Soft-Industrial." Elements are generally rectangular to maximize screen real estate for data, but with a slight 0.25rem (4px) radius to prevent the UI from feeling overly aggressive or "brutalist."

*   **Standard Elements:** 4px radius (Buttons, Input fields, Cards).
*   **Large Containers:** 8px radius (Main dashboard panels).
*   **Icons:** Contained within square or hexagonal bounding boxes.

## Components

*   **Buttons:**
    *   *Primary:* Solid `#F59E0B` with `#0A0A0A` text. No gradients.
    *   *Secondary:* Ghost style with `#1F2937` border and `#FAFAF9` text. 
*   **Input Fields:** Dark background (`#0A0A0A`), 1px border (`#374151`), JetBrains Mono for input text. On focus, the border transitions to Primary Amber.
*   **Cards:** Use Level 1 Surface (`#111111`). Top-right corner may feature a subtle monospaced "ID" or "Tag" for a technical feel.
*   **Chips/Tags:** Monospaced text, small font size (12px), background matches the border color (`#1F2937`) with 50% opacity.
*   **Code Blocks:** Integrated JetBrains Mono text. Syntax highlighting should use a limited palette of Amber, Slate, and Cream to maintain the monochromatic aesthetic.
*   **Status Indicators:** Use the hexagonal motif—a small 8px hex icon that pulses with Amber for "Live" states or Slate for "Inactive."