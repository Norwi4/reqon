# REQON — Website Brand & Design Guidelines
Version: 1.0 | Language: RU | Status: proposed design system

## 1. Brand foundation
- Product: **Reqon** (capital R, lowercase eqon). Never Getman in public UI.
- Category: desktop API client for developers; HTTP, gRPC and other protocols as actually supported.
- Positioning: a powerful, focused workspace to send requests, inspect responses and organize API workflows.
- Brand personality: technically credible, clear, fast, independent, friendly without being childish.
- Mascot: stylized raccoon. Use the simplified geometric raccoon as product icon; full character illustration only in storytelling, onboarding, About and marketing.
- English headline proposal: **Every request. Under your control.**
- Supporting line: **Build, test and explore APIs in one focused desktop workspace.**
- Russian proposal: **Каждый запрос — под твоим контролем.**
- Tone: direct, concise, helpful; no unsupported claims (e.g., fastest, secure-by-default, free forever).

## 2. Design principles
1. Product first: real screenshots and clear features outrank decorative illustrations.
2. Dark-first, with legible contrast and restrained orange emphasis.
3. Dense enough for developers, spacious enough for marketing.
4. Distinct from Postman: borrow information architecture, not layout, text, artwork or trade dress.
5. Accessible and responsive, not animation-heavy.

## 3. Color system
Core tokens (hex):
| Token | Value | Usage |
|---|---|---|
| `--color-bg` | `#10161D` | page background |
| `--color-surface` | `#1C242D` | cards, header |
| `--color-surface-raised` | `#263847` | hover, raised blocks |
| `--color-border` | `#33404D` | dividers, borders |
| `--color-accent` | `#FF8319` | primary CTA, highlight |
| `--color-accent-hover` | `#FF9B42` | hover |
| `--color-text` | `#F5F7FA` | main text |
| `--color-muted` | `#B6C2CF` | secondary text |
| `--color-subtle` | `#8998A8` | metadata, only where contrast passes |
| `--color-success` | `#49C995` | success |
| `--color-warning` | `#F3B65C` | warnings |
| `--color-error` | `#F47777` | errors |

Use orange for primary actions, selected accents, tiny highlights; avoid orange body text and large solid orange backgrounds. Dark text `#10161D` on orange buttons. Ensure WCAG AA contrast for text (4.5:1 normal, 3:1 large), controls and focus indicators (3:1 where applicable). Verify actual combinations in implementation. Optional light mode is a future extension, not a required v1 feature.

## 4. Typography
- Primary: **Inter** (self-hosted or next/font, with appropriate license).
- Code: **JetBrains Mono** (self-hosted or next/font).
- Fallback: `Inter, system-ui, -apple-system, Segoe UI, sans-serif`.
- Hero: 56–72px desktop, 36–44px mobile; weight 700–800; line-height 1.05–1.12.
- H1: 48–60px; H2: 36–44px; H3: 24–28px; body: 16–18px / line-height 1.5–1.7; small: 13–14px.
- Code samples: 13–15px, monospaced, syntax highlighting with readable contrast.
- Use sentence case; no all-caps paragraphs; avoid gratuitous gradients in text.

## 5. Identity and assets
- Main application icon: **flat geometric raccoon** with white face/black mask/orange goggles on deep navy background. It must read at 16–24px.
- Marketing mascot: detailed raccoon in orange goggles and dark hoodie, for About, hero secondary illustration, empty states.
- Use assets from the **Reqon Developer Icons v2** package as the source of truth; do not redraw or substitute logos.
- Brand wordmark: `Reqon`, bold Inter or an approved custom wordmark, in white on dark.
- Monogram `R` is secondary, not a substitute for the raccoon product icon.
- Keep aspect ratio, clear space (at least 1/4 icon width around logo in marketing layouts); no distortion, shadow overload, or recoloring without a deliberate variant.
- Include favicon, apple-touch-icon, Open Graph image, and app download assets when available. Check actual package paths before referencing them.

## 6. Layout and spacing
- 12-column desktop grid; max content width 1200–1280px; horizontal padding 24–40px desktop, 16–20px mobile.
- Spacing scale: 4, 8, 12, 16, 24, 32, 48, 64, 96px.
- Radius: 8px controls, 12–16px cards, 20–24px major showcase containers; pills only for badges.
- Borders: 1px `--color-border`; shadows subtle, never glossy/neumorphic.
- Header: compact sticky nav, logo left, Features / Download / Docs / Changelog, primary Download CTA; mobile menu accessible.
- Hero: short headline, concise subheading, Windows download CTA and secondary Explore features CTA, genuine app screenshot, OS labels.
- Page rhythm: strong hero → supported protocol/features → screenshot-led walkthrough → workflow advantages → download CTA → footer.

## 7. UI component rules
- Primary button: accent orange background, dark text, 10–12px vertical and 18–22px horizontal padding; radius 8–10px; visible focus ring.
- Secondary button: transparent/dark surface with visible border, white text.
- Cards: surface background, subtle border, 12–16px radius, 24–32px padding; avoid endless identical card grids.
- Links: text underlines on hover/focus, not color alone as signal.
- Icons: Lucide outline icons for UI; consistent 20–24px stroke and scale. Brand raccoon only for brand use.
- Code and request/response demos: use authentic-looking API data, but clearly mark illustrative examples; do not claim fake requests are live.
- Motion: 150–250ms transitions; honor `prefers-reduced-motion`.
- Navigation and CTAs must work, not be decorative placeholders.

## 8. Site map (MVP)
- `/` — product value, supported features, real screenshots, download CTA.
- `/features` — HTTP, gRPC, collections, environments, history, other *verified* features.
- `/download` — releases for actually supported OS/architectures, version, date, size, SHA-256, release notes.
- `/docs` — installation, quickstart, first request, collections, environments, gRPC.
- `/changelog` — release history and changes.
- `/about` — product story, mascot, project identity and credits.
- Header + footer links, 404 page, metadata, robots and sitemap.
- Optional RU/EN localization; if implemented, use locale-aware routes and correct hreflang.

## 9. Content style
- Prefer benefits backed by product screenshots: 'Send HTTP requests', 'Explore gRPC services', 'Keep requests organized'.
- Avoid copying Postman copy or illustrations.
- Never promise cloud sync, teams, authentication, AI or protocols unless built.
- Keep headings to one thought; body copy short and scannable.
- Product UI should show real Reqon screenshots (supplied separately) rather than generated imitations.

## 10. Technical guidance for site implementation
- Suggested stack: Next.js + TypeScript + Tailwind CSS + MDX for docs.
- Implement semantic tokens as CSS custom properties and map them into Tailwind theme; avoid scattered hardcoded colors.
- Reusable components: SiteHeader, SiteFooter, Button, FeatureCard, ScreenshotFrame, DownloadCard, CodeSnippet, Callout.
- Responsive breakpoints: mobile <640px, tablet 640–1023px, desktop >=1024px (adapt to framework).
- Accessibility: semantic landmarks, keyboard navigation, focus states, alt text, skip link, reduced motion.
- Performance: optimized image sizes, no giant hero video by default, font subset/preload, proper image dimensions.
- SEO: unique titles/descriptions, canonical URLs, Open Graph cards, sitemap, robots.txt, structured data only when valid.
- Download links and version data must come from verified release metadata; do not fabricate binaries, checksums or dates.

## 11. Example design tokens
```css
:root {
  --color-bg: #10161D;
  --color-surface: #1C242D;
  --color-surface-raised: #263847;
  --color-border: #33404D;
  --color-accent: #FF8319;
  --color-accent-hover: #FF9B42;
  --color-text: #F5F7FA;
  --color-muted: #B6C2CF;
  --color-success: #49C995;
  --color-warning: #F3B65C;
  --color-error: #F47777;
  --font-sans: 'Inter', system-ui, sans-serif;
  --font-mono: 'JetBrains Mono', ui-monospace, monospace;
  --radius-control: 10px;
  --radius-card: 16px;
  --container-max: 1240px;
}
```

## 12. Prompt integration instructions
Treat this document as the authoritative **visual and content brand specification** for the Reqon website. Build the website using these tokens, typographic rules, logo conventions, layout principles and content style. Inspect available assets before using them. When implementation requirements conflict with these recommendations, preserve accessibility, real product behavior and factual accuracy, and explain any necessary deviations. Do not confuse Reqon with its former internal working name Getman.

## 13. Items requiring real project inputs
- Final domain and social links.
- Actual supported platforms, install files, release/version information.
- Actual product screenshots.
- Source brand icon pack paths and optional mascot art.
- Legal pages/contact details, if publishing publicly.
