# Third-party notices

## HerdrAgentIconsMax

This plugin bundles `assets/fonts/HerdrAgentIconsMax-Regular.ttf`, copied from
[hhdebb/herdr-radar](https://github.com/hhdebb/herdr-radar),
`dist/HerdrAgentIconsMax-Regular.ttf` (SHA-256
`1b8199496b223d0761fc3a49d5b2510bda38deab82cb3758bf3980495bf05272`).
The font originates in
[qintmb/herdr-icon-agent-ui](https://github.com/qintmb/herdr-icon-agent-ui)
and herdr-kit contributors under the MIT license reproduced below.
JetBrains Mono and JetBrainsMonoHerdr are not bundled; this icon font contains
no JetBrains Mono outlines.

The font includes U+E1A0–U+E1BA vendor marks and U+E1C0–U+E1C5 state glyphs.
Lineup maps only U+E1A0–U+E1BA; the state glyphs remain reserved and unused.
Installation and terminal mapping require the explicit opt-in `install-font`
action. Startup never installs fonts or changes terminal configuration.

Marks for Cursor, OpenCode, Hermes, Copilot, DeepSeek, Gemini, GPT, Qwen,
Antigravity, Kiro, GLM, and Muse derive from
[lobehub/lobe-icons](https://github.com/lobehub/lobe-icons) (MIT), re-normalized
to bare SVG path geometry by upstream. Its MIT license covers the project's
packaging, not the depicted brands' trademarks. Other mark sources and licenses
are recorded in the upstream
[third-party notices](https://github.com/hhdebb/herdr-radar/blob/main/THIRD_PARTY_NOTICES.md).

Vendor names and logos are trademarks of their respective owners. Their use
identifies supported products and does not imply affiliation, endorsement, or
partnership. The MIT license covers the font project's code and packaging,
not rights to vendor trademarks.

The following is the full [upstream MIT license](https://github.com/hhdebb/herdr-radar/blob/main/LICENSE):

```text
MIT License

Copyright (c) 2025 qintmb
Copyright (c) 2026 herdr-kit contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## herdr-agent-quota statusLine schema handling

The active-model quota-pool detection and statusLine field interpretation in
`src/statusline.rs` are adapted from the MIT-licensed
[fujimogn/herdr-agent-quota](https://github.com/fujimogn/herdr-agent-quota),
commit `d428889cd6d0dc675e0b4058dc5c5f172d979e5b`, particularly
`src/providers/agy.rs`, `src/providers/claude.rs`, and the shared parser.
The Lineup implementation adds bounded capture, independent missing outcomes,
exact session/pane associations, and rejects invalid ranges rather than
clamping them. Credential/account readers were not copied or enabled.

```text
MIT License

Copyright (c) 2026 Levi Qiao

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## herdr-agent-quota Codex app-server protocol

The request sequence and primary/secondary window interpretation in
`src/quota_codex.rs` are adapted from `src/providers/codex.rs` in
[fujimogn/herdr-agent-quota](https://github.com/fujimogn/herdr-agent-quota),
commit `d428889cd6d0dc675e0b4058dc5c5f172d979e5b`.
Copyright (c) 2026 Levi Qiao. The MIT license reproduced in the preceding
herdr-agent-quota notice applies. Lineup adds bounded nonblocking I/O,
subscription-only weekly selection, sanitized errors and a reset-bounded
in-memory cache. Authentication-file and conversation readers were not copied.

## herdr-agent-quota Grok billing schema

The request headers and proto3 omitted-zero billing interpretation in
`src/quota_grok.rs` are adapted from `src/providers/grok.rs` in
[fujimogn/herdr-agent-quota](https://github.com/fujimogn/herdr-agent-quota),
commit `d428889cd6d0dc675e0b4058dc5c5f172d979e5b`.
Copyright (c) 2026 Levi Qiao. The MIT license reproduced in the preceding
herdr-agent-quota notice applies.
