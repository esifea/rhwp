# Edited document export comparison

These public sample images were rendered on 2026-10-29 with the final implementation. The Native and freshly built WASM document rasters are byte-identical. Review-panel headers identify the backend and therefore differ. These are renderer-to-independent-PDF comparisons, not before/after screenshots or a pixel-perfect compatibility claim.

| Image prefix | Input | Independent reference | Page | Tolerant content match |
| --- | --- | --- | --- | --- |
| table-continuation | `samples/issue1133/page-number-footer15.hwpx` | `pdf/issue1133/page-number-footer15-2020.pdf` | 3 | 99.23558% |
| font-limitation | `samples/task1749/saved_bounds_cumulative_page_break.hwp` | `samples/task1749/saved_bounds_cumulative_page_break-2024.pdf` | 3 | 85.37702% |

The table continuation preserves the blank left cell, right-cell note, physical borders and following rows. The preceding part remains on page 2. Both margin variants retain three pages. Formal regressions check ownership, text order, borders and following content.

The legacy page is retained to disclose a limitation. An independent Chrome CDP audit found substitute faces on all 715 visible font-painted nodes: Pretendard (113), NotoSerifKRExtraLight (599), and NotoSerifCJKkr (3), instead of the PDF's HaansoftBatang, HaansoftDotum, H2gtrE, H2mjsM and HumanMyeongJo faces. One additional invisible accessibility node accompanies a vector middle dot. All 24 nonempty lines and 716 nonspace glyphs retain content and order. Paragraph starts differ by at most 0.0971px and baselines by at most 0.08px; this page contains no table or picture frame. Footer baseline is 1072.618645833px versus the independent 1072.64px. Coordinates use the source 96dpi plane, not whole-paper projection.

The score exception is limited to that page and actual substitute-font evidence. The score remains below 90%; no new visual regression is added for it. Two minor non-font residuals remain: the generic vector middle dot is about 0.73px wider and 0.64px lower, and the 595.28x841.88pt source page versus 595x841pt PDF creates a blank one-pixel paper edge. No fonts or PDF font subsets are bundled. New-test pages independently meet the required visual threshold.

The combined source is based on `1d18ef209ddf1920a3335baa4e1e37f77dea3c88`; remote freshness was not rechecked during this validation. The authoritative implementation snapshot has SHA-256 `3cf0a43976e4f3a801573f99f34ab7ac36660910cd125879fa11e5ad70ed0739`. Frozen Native binary: `1e7eff1f55d97d888f4ce7484c1104580462a7e517cce426852e186b17e70233`; fresh WASM: `9109f23a342f867c6ed5240bf148d806fe7505d7d32380e8b14d7ce02a69d676`. Both actual compiler builds reported `fresh:false`. Adding tests or these evidence files does not alter that implementation snapshot.

Run the repository's `scripts/visual_sweep.py` with these inputs, reference PDFs and page selections for each backend. Exact commands, input hashes and full local manifests accompany the prepared review series. The font exception evidence was hashed as `a4179579bcbcfbadfcf6cdeea9fa2f89c665d71194cbabdf393c256ba8e48bb1`. That evidence records fresh final captures and byte identity to the independently audited font/geometry output.

Before opening a PR, use the actual committed head SHA in the image URLs. These images do not establish that the changes have been deployed in Fhellow.
