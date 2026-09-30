# Theme tokens

Use shared/theme.css as the runtime source of truth.

- Theme: graphite dark by default, independently designed light theme; persist the selection locally.
- Accent: teal, used for primary actions, focus and selected navigation. Functional status colors always include text.
- Spacing: 4/8/12/16/24px; page 12px, compact 8px.
- Controls 32px; table rows 36px; header 44px; coarse pointer controls 44px.
- Text: 12px secondary, 13px body, 18px page title; system CJK sans; system monospace for versions and paths; tabular numerals.
- Radius: 4px controls, 8px panels, 12px dialogs.
- Motion: color/opacity only, 80–150ms; honor reduced motion.
- Icons: Lucide SVG; important actions retain text labels.

## Control and information hierarchy

- Normal actions use a distinct, subdued surface with a faint border; avoid bright outlines around every control. Table actions are 28px high; danger actions use a soft tinted surface. Quiet styling is suitable for conventional icon-only navigation or window controls.
- Mutually exclusive settings use SegmentedControl: one faint outer boundary, no strong internal dividers, native radios with visible empty/filled circles and selected fill. Multiple selection uses checkboxes. Only active navigation tabs have an accent underline; add-project/version/platform entries use a faint dashed border.
- Saved server entries are borderless at rest; only the selected entry has an accent border and tint. Inputs retain a faint border and an inset surface. Strong outlines appear on keyboard focus, not on every resting control.
- Immediate boolean settings use ToggleSwitch, with the setting name and explicit 开启/关闭 text. Remote changes update only after server success.
- Read-only status uses StatusBadge with a semantic icon and word; status badges are not styled as buttons. Published rows use only a subtle tint.
- Typography: body/data 13px regular; labels 13px medium; secondary 12px; key identifiers 13–14px semibold; result metric 16px semibold; dialog title 16px; page title 18px.
- Disabled actions have explicit styling and a reason when not obvious. ActionButton exposes reasons on hover and keyboard focus; Escape dismisses the visual tooltip.
- Normal text targets contrast 4.5:1; radio/switch state indicators and keyboard focus remain distinct in both themes. Decorative resting borders use the subdued `--control-border`; radio marks use the stronger `--control-indicator`. Do not brighten every border to compensate for weak hierarchy. Check disabled appearance and the complete composition visually; do not claim full accessibility compliance from token checks.
