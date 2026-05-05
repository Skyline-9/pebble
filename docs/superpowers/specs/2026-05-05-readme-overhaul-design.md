# Design Spec: README Overhaul for GitHub Trending

**Date:** 2026-05-05
**Status:** Approved
**Topic:** Overhauling the `pebble` README to capture the "Agentic Memory" trend and build authority.

## 1. Goal
Optimize the repository's main entry point (README.md) to attract stars and interest from the GitHub Trending community by emphasizing the project's research pedigree, unique architecture, and competitive advantages.

## 2. Content Strategy: "The New Paradigm"
We will shift from a purely technical description to a narrative-driven "hacker-style" README.

### Key Sections:
1.  **Hero:** Standardized badges (License, Bun, MCP) and a punchy "Context Compression" value proposition.
2.  **Architecture (Mermaid.js):** A professional `flowchart LR` or `sequenceDiagram` showing the `Log -> SQLite -> Hot Cache` loop. This replaces the ASCII diagram.
3.  **The Research Pedigree:** Explicitly cite the papers that inspired the design (EverMemOS, AutoSkill, SwiftMem) to build authority.
4.  **Competitive Edge (vs. Claude Obsidian):** A comparison highlighting Pebble's "Agent-Native" and "Append-Only" nature vs. simple file-syncing tools.
5.  **Features Grid:** A table or list of "Known-Good" research patterns implemented (AutoSkill extraction, belief revision, etc.).
6.  **Quickstart:** Minimal, high-energy instructions with emojis for visual guidance.

## 3. Visuals & Formatting
*   **Mermaid.js:** Used for the primary architectural visualization.
*   **Admonitions:** Use GitHub's `> [!NOTE]` or `> [!TIP]` blocks for key highlights.
*   **Emoji Usage:** Strategic use of emojis in headers to improve scanability (🚀, 🧠, 🗄️, 📂).

## 4. Success Criteria
*   README feels "premium" and "research-backed".
*   Unique selling points (Local-first, Research-based, MCP) are visible within 5 seconds of scrolling.
*   Setup instructions are clear and copy-paste friendly.

## 5. Non-Goals (for this phase)
*   Flashy SVG/GIF hero visuals (deferred per user request).
*   Detailed documentation for every sub-package (keep it focused on the core narrative).
