# Herdr plugin conventions

Research snapshot: 2026-10-12. This compares public Herdr plugin packaging with
`herdr-lineup` at `911a260e40e73bd7260089347a95cb2d041c6729` (`origin/main` when
this worktree was created). It does not change the manifest, CI, or runtime.
As of tag v0.1.1 (`cacef43efab070cd17e5a5f6448aabc5bdb039dc`, 2026-10-14), the repository is public and the README documents GitHub installation. Statements below that public installation is planned, or that the repository stays private, describe this 2026-10-12 snapshot.

## Method and boundaries

Facts below come from pages and files retrieved on 2026-10-12 over
unauthenticated HTTPS. `gh` was not used: the local GitHub CLI would attach
stored credentials, and this survey was limited to public reads. No plugin was
installed, linked, or executed. No font binary was decoded.

The formation assignment referred to a brief that already surveyed ten Rust
plugins. No such brief is in this repository or in the formation event log.
The Rust cohort below is this survey's selection, not a recovered list. The
rule was: the highest-star rows whose marketplace language is Rust, plus
`zackshen/herdr-workspace` (same install-script shape at low star count) and
the official cookbook's Rust example. Non-Rust rows were chosen because they
are sidebar, usage, font, or marketplace neighbors of lineup. This is not a
census of the index.

Each inspected repository is pinned to the marketplace index `headCommit`
except `ogulcancelik/herdr-plugin-examples`. That repository was absent from
the index snapshot. Its files were read from raw `main` after the
unauthenticated GitHub REST budget was exhausted, so that row has a retrieval
URL and date but no commit SHA. Treat that row as the bytes returned by those
URLs on this date, not as a permanent pin.

`fujimogn/herdr-agent-quota` does not appear in the public index snapshot.
This survey did not call an authenticated API to learn whether the repository
is private. Usage parsing for that repository is a separate note in
[usage-sources.md](usage-sources.md).

Inference is marked **Inference**. Everything else in the tables is a reading
of the cited page or file.

## Marketplace listing

Official conditions, read on 2026-10-12:

- [Marketplace](https://herdr.dev/docs/marketplace/): the index lists public
  GitHub repositories tagged `herdr-plugin` when the default branch contains
  at least one `herdr-plugin.toml` whose required metadata parses. It is not a
  reviewed catalog. One card per repository; each valid manifest is a separate
  install. Forks, archived repositories, repositories without a valid manifest,
  and malformed metadata are excluded. The index refreshes about every 30
  minutes and when the default-branch head changes. A card shows repository
  metadata; plugin rows show `name` and `version`. The index also stores the
  manifest path, `id`, `platforms`, `min_herdr_version`, and the head commit.
- [Plugins](https://herdr.dev/docs/plugins/): required manifest fields are
  `id`, `name`, `version`, and `min_herdr_version`. `description` is optional.
  `plugin install` clones GitHub shorthand `owner/repo[/subdir]`, runs
  `[[build]]`, and replaces the managed checkout on reinstall. There is no
  `plugin update`. `plugin link` does not run build commands. Config belongs
  in `HERDR_PLUGIN_CONFIG_DIR` and runtime state in `HERDR_PLUGIN_STATE_DIR`.
  Do not store credentials or durable state in `HERDR_PLUGIN_ROOT`. There is
  no Herdr-managed storage API. Startup hooks are one-shot, not supervised
  daemons.
- The plugins page at <https://herdr.dev/plugins/> loads
  <https://assets.herdr.dev/plugins/index.json>.

Index snapshot `generatedAt` `2026-10-12T06:01:04.815Z`, `schemaVersion` 1,
`source.query` `topic:herdr-plugin is:public`:

| Field | Value |
| --- | --- |
| `totalCount` / `collectedCount` | 1675 / 1675 |
| `repositoryCount` / `pluginCount` | 1618 / 1673 |
| `missingManifestCount` | 38 |
| `invalidManifestCount` | 4 |
| `duplicateManifestCount` | 7 |
| `blacklistedCount` | 2 |
| `unavailableRepositoryCount` | 1 |
| `truncated` | false |

An unauthenticated search of `topic:herdr-plugin` on the same date also
reported `total_count` 1675. That search counts topic tags, not parsed
manifests. Primary language counts in the index snapshot: Rust 361, Python
331, Shell 327, JavaScript 220, Go 178, TypeScript 149, and smaller counts for
other languages. **Inference:** a topic tag alone does not mean the repository
is installable; the index exclusions above are the filter.

## Cohort

Permalink shape for a pinned row:
`https://github.com/<repo>/tree/<sha>`. Manifest URL shape:
`https://github.com/<repo>/blob/<sha>/<path>`.

| Repository | Language | Stars | Head | Manifest path | Retrieved |
| --- | --- | --- | --- | --- | --- |
| [furkankly/zoetrope](https://github.com/furkankly/zoetrope/tree/b1f31dd26bd4) | Rust | 1029 | `b1f31dd26bd4` | `herdr-plugin/herdr-plugin.toml` | 2026-10-12 |
| [persiyanov/herdr-reviewr](https://github.com/persiyanov/herdr-reviewr/tree/859dd5b11a52) | Rust | 869 | `859dd5b11a52` | `herdr-plugin.toml` | 2026-10-12 |
| [plannotator/herdr-annotate](https://github.com/plannotator/herdr-annotate/tree/ceab050b77be) | Rust | 654 | `ceab050b77be` | `herdr-plugin.toml`, `lite/herdr-plugin.toml` | 2026-10-12 |
| [smarzban/herdr-file-viewer](https://github.com/smarzban/herdr-file-viewer/tree/3996b88d6c6c) | Rust | 642 | `3996b88d6c6c` | `herdr-plugin.toml` | 2026-10-12 |
| [eliasstravik/herdr-projects](https://github.com/eliasstravik/herdr-projects/tree/4e4548c3c438) | Rust | 603 | `4e4548c3c438` | `herdr-plugin.toml` | 2026-10-12 |
| [alexarthurs/herdr-sidebar](https://github.com/alexarthurs/herdr-sidebar/tree/081ed133f3fa) | Rust | 462 | `081ed133f3fa` | `plugins/herdr-sidebar/herdr-plugin.toml` | 2026-10-12 |
| [nelsonPires5/herdr-board](https://github.com/nelsonPires5/herdr-board/tree/754c836d881c) | Rust | 173 | `754c836d881c` | `herdr-plugin.toml` | 2026-10-12 |
| [natori-hrj/herdr-lazy](https://github.com/natori-hrj/herdr-lazy/tree/8caad442f371) | Rust | 27 | `8caad442f371` | `herdr-plugin.toml` | 2026-10-12 |
| [zackshen/herdr-workspace](https://github.com/zackshen/herdr-workspace/tree/b5f38d0e2951) | Rust | 2 | `b5f38d0e2951` | `herdr-plugin.toml` | 2026-10-12 |
| [ogulcancelik/herdr-plugin-examples](https://github.com/ogulcancelik/herdr-plugin-examples/blob/main/rust-release-check/herdr-plugin.toml) | Rust example | not in index | raw `main`, SHA not pinned | `rust-release-check/herdr-plugin.toml` | 2026-10-12 |
| [hhdebb/herdr-radar](https://github.com/hhdebb/herdr-radar/tree/dd42772da4b8) | JavaScript | 166 | `dd42772da4b8` | `herdr-plugin.toml` | 2026-10-12 |
| [senna-lang/herdr-agent-usage](https://github.com/senna-lang/herdr-agent-usage/tree/fc01eab38a12) | Go | 48 | `fc01eab38a12` | `herdr-plugin.toml` | 2026-10-12 |
| [cloudmanic/herdr-plus](https://github.com/cloudmanic/herdr-plus/tree/f38df3570bea) | Go | 350 | `f38df3570bea` | `herdr-plugin.toml` | 2026-10-12 |
| [kryptamine/herdr-auto-title](https://github.com/kryptamine/herdr-auto-title/tree/a5a2701d352d) | Go | 249 | `a5a2701d352d` | `herdr-plugin.toml` | 2026-10-12 |
| [jhochenbaum/herdr-hunk-diff](https://github.com/jhochenbaum/herdr-hunk-diff/tree/47146a058858) | TypeScript | 143 | `47146a058858` | `herdr-plugin.toml` | 2026-10-12 |
| [aigorahub/herdr-lantern](https://github.com/aigorahub/herdr-lantern/tree/c44b740648e9) | Python | 68 | `c44b740648e9` | `herdr-plugin.toml` | 2026-10-12 |
| [qintmb/herdr-icon-agent-ui](https://github.com/qintmb/herdr-icon-agent-ui/tree/82eac5161a8b) | Python | 14 | `82eac5161a8b` | `herdr-plugin.toml` | 2026-10-12 |
| [speardragon/herdr-plugin-manager](https://github.com/speardragon/herdr-plugin-manager/tree/e4fef2693728) | Shell | 55 | `e4fef2693728` | `herdr-plugin.toml` | 2026-10-12 |
| [eliasstravik/herdr-call](https://github.com/eliasstravik/herdr-call/tree/c722bcd58b1c) | TypeScript | 11 | `c722bcd58b1c` | `herdr-plugin.toml` | 2026-10-12 |
| [BlockedPath/herdr-agent-icons](https://github.com/BlockedPath/herdr-agent-icons/tree/46d199492476) | Shell | 0 | `46d199492476` | `herdr-plugin.toml` | 2026-10-12 |
| [ogulcancelik/herdr-plugin-examples `agent-telegram-notify`](https://github.com/ogulcancelik/herdr-plugin-examples/blob/main/agent-telegram-notify/herdr-plugin.toml) | JavaScript | not in index | raw `main`, SHA not pinned | `agent-telegram-notify/herdr-plugin.toml` | 2026-10-12 |
| [ogulcancelik/herdr-plugin-examples `dev-layout-bootstrap`](https://github.com/ogulcancelik/herdr-plugin-examples/blob/main/dev-layout-bootstrap/herdr-plugin.toml) | Lua | not in index | raw `main`, SHA not pinned | `dev-layout-bootstrap/herdr-plugin.toml` | 2026-10-12 |
| [ogulcancelik/herdr-plugin-examples `github-link-preview`](https://github.com/ogulcancelik/herdr-plugin-examples/blob/main/github-link-preview/herdr-plugin.toml) | Bash | not in index | raw `main`, SHA not pinned | `github-link-preview/herdr-plugin.toml` | 2026-10-12 |

The three cookbook manifests above were read from raw `main` on 2026-10-12.
The same URL shape was used for `rust-release-check`. The cookbook README says
Herdr does not maintain them as official plugins; the plugins doc points at
the same repository as examples to copy. Their commit SHA was not pinned.

Stars are the index field `stars` at `generatedAt`, not a trust score.
`herdr-plugin-examples` has no index star count because it was not listed.

## Manifest shape

Required fields were present on every manifest opened. `platforms` was also
present on every opened manifest. Ids are either a bare name (`herdr-lineup`,
`herdr-projects`, `annotate`) or a dotted owner-qualified id
(`furkankly.zoetrope`, `persiyanov.reviewr`, `hhdebb.herdr-radar`,
`examples.rust-release-check`). The plugins doc allows both: ASCII letters,
digits, dot, colon, underscore, and hyphen.

Build commands are argv arrays. Patterns observed:

| Pattern | Where it was read |
| --- | --- |
| `cargo build --release` with no `--locked` | `rust-release-check/herdr-plugin.toml` |
| `sh` installer that downloads a release, checks `SHA256SUMS`, and runs `cargo build --release --locked` on failure | `herdr-projects` `scripts/install.sh` |
| Download a release asset and check a `.sha256` sidecar; the script tells the user to run `cargo build --release` when no asset matches, and does not pass `--locked` in that message | `herdr-reviewr` `herdr/install.sh`; `herdr-workspace` `herdr/install.sh` |
| `fetch-or-build` shell plus a Windows PowerShell twin | `herdr-file-viewer`, `herdr-sidebar`, `herdr-lazy` |
| `go build` in the manifest, split by `platforms` | `herdr-auto-title`, Windows half of `herdr-plus` |
| `npm ci` then `npm run build` | `herdr-call` |
| `node` setup or build scripts | `herdr-radar` `bin/setup.js`; `herdr-hunk-diff` |
| No `[[build]]` | `herdr-lantern`, `herdr-plugin-manager`, `herdr-icon-agent-ui`, `herdr-agent-icons` |

`herdr-projects` startup runs `target/release/herdr-projects`. `herdr-lazy`
startup runs `./target/release/herdr-lazy`. That matches a source build that
leaves the binary in `target/release`. Fetch-or-build plugins instead point
later commands at a downloaded binary path. `herdr-projects`
`scripts/install.sh` is the one opened installer that both verifies
`SHA256SUMS` and invokes `cargo build --release --locked` itself.
`herdr-reviewr` and `herdr-workspace` verify a checksum sidecar and, when no
binary matches, print a `cargo build --release` instruction. They do not pass
`--locked` in that message. **Inference:** a fetch script lets install skip a
toolchain when a release asset matches. The projects script states that
fallback; the other two scripts were not executed, so their miss path is the
printed command only.

Several manifests split `platforms` per item because Herdr needs a different
argv on Windows (`herdr-reviewr`, `herdr-file-viewer`, `herdr-sidebar`,
`herdr-plus`, `herdr-auto-title`). `herdr-auto-title` comments that a second
action with the same id is rejected even on another platform, so the Windows
restart action is a single cross-platform id whose `.exe` suffix Windows
resolves. `herdr-plus` uses distinct `-windows` ids instead.

Startup is common for sidebar publishers (`herdr-radar` three startup
commands, `herdr-agent-usage` `bin/run-startup.sh`, `herdr-icon-agent-ui`
`run.sh` plus `agent_state.py`) and absent for pane-only tools such as
`herdr-file-viewer` and `herdr-reviewr`. `herdr-lazy` comments that its
`worktree.created` hook only records context under `HERDR_PLUGIN_STATE_DIR`
and does not install or edit the plugin list.

`herdr-hunk-diff` is the only inspected manifest with `[[link_handlers]]`
besides the cookbook's `github-link-preview`.

Version drift is real. At the inspected heads, `zoetrope`'s plugin manifest
`version` is `0.1.0` while the newest release tag returned by the releases API
was `v0.2.0`. `herdr-annotate`'s opened manifests say `0.9.1` while the newest
release tag was `rust-lite-v0.2.0`. Those are two different reads (manifest
file versus releases API); this survey did not check out the release tags.

## README shape

Headings are not standardized. Recurring sections in the files opened:

- One-line purpose, then requirements (`herdr` minimum, OS, toolchain only if
  building from source). `herdr-reviewr`, `herdr-workspace`, and
  `herdr-file-viewer` do this.
- Install command as fully qualified `herdr plugin install owner/repo[/subdir]`.
  Subdirectory installs are required for `zoetrope` (`furkankly/zoetrope/herdr-plugin`)
  and `herdr-sidebar` (`alexarthurs/herdr-sidebar/plugins/herdr-sidebar`).
- A note that `plugin link` does not run `[[build]]`, so local development
  builds first. The plugins doc says this. `herdr-icon-agent-ui` and the
  cookbook README repeat it.
- Configuration via `herdr plugin config-dir <plugin-id>`, not a hardcoded
  path only. Seen in `herdr-agent-usage`, `herdr-radar`, `herdr-file-viewer`,
  `herdr-lazy`, `herdr-plus`, and `herdr-icon-agent-ui`.
- Screenshots or a "what it looks like" section on radar, reviewr, file-viewer,
  and sidebar. Lineup's README says no runtime screenshot exists yet.
- License and, for icon fonts, third-party notices (`herdr-radar`
  `THIRD_PARTY_NOTICES.md`, `herdr-icon-agent-ui` `THIRD_PARTY_NOTICES.md`).

`herdr-projects` and `herdr-call` READMEs are marketing pages (benefit
headings and FAQ) with less command-level structure. **Inference:** that
shape fits a product pitch more than a scaffold. It is not a packaging
requirement.

`herdr-plugin-manager`'s README describes marketplace browsing as a GitHub
topic search inside the popup. That matches the official index rule. The
README text was not re-verified against the script's source beyond the
heading list.

## Tags, releases, and CI

Releases and tags were read from the public GitHub REST resources
`/repos/<owner>/<repo>/releases?per_page=5` and `/tags?per_page=8` on
2026-10-12. Workflow paths come from
`/repos/<owner>/<repo>/git/trees/<head>?recursive=1`. Workflow YAML bodies
were not opened, so job steps are not claimed.

| Repository | Newest tag seen | Newest release assets include | Workflow paths |
| --- | --- | --- | --- |
| zoetrope | `v0.2.0` | per-target `.tar.gz` plus `.sha512` and `.asc` | `ci.yml`, `cd.yml`, `release-plz.yml`, `website.yml` |
| herdr-reviewr | `v0.46.0` | per-target archives plus `.sha256` sidecars | `ci.yml`, `audit.yml`, `release.yml` |
| herdr-annotate | `rust-lite-v0.2.0` | bare binaries plus `SHA256SUMS` | `rust-lite-ci.yml`, `rust-lite-release.yml`, `windows-full-ci.yml` |
| herdr-file-viewer | `v1.18.0` | bare binaries plus `SHA256SUMS` | `ci.yml`, `release.yml` |
| herdr-projects | `v0.2.34` | bare binaries plus `SHA256SUMS` | `release.yml` only |
| herdr-sidebar | `v0.15.1` | bare binaries plus `SHA256SUMS` | `ci.yml`, `release.yml`, `agent-command-guard.yml` |
| herdr-board | `v0.19.0` | one Linux `.tar.gz` plus `.sha256` | `ci.yml`, `release.yml`, `prepare-release.yml`, `promote.yml` |
| herdr-lazy | `v0.44.0` | per-target `.tar.gz` plus `.sha256` | `ci.yml`, `release.yml` |
| herdr-workspace | `v0.1.2` | archives plus `.sha256` sidecars | `release.yml` only |
| herdr-radar | `v1.4.4` | release object with an empty asset list | `ci.yml` |
| herdr-agent-usage | `v0.5.20` | Go binaries plus `SHA256SUMS` | `ci.yml`, `release.yml`, plus contract, lint, model, and review workflows |
| herdr-plus | `v0.1.24` | Go archives plus `checksums.txt` | `test.yml`, `release.yml`, `site.yml` |
| herdr-auto-title | `v0.13.1` | release object with an empty asset list | `ci.yml`, `release-please.yml` |
| herdr-hunk-diff | `v0.4.0` | empty asset list | `ci.yml`, `sync-verified-hunk.yml` |
| herdr-lantern | `v0.17.8` | empty asset list | `tests.yml`, `pages.yml` |
| herdr-icon-agent-ui | `v2.1.0` | empty asset list | no `.github/workflows/` blob in the tree |
| herdr-plugin-manager | `v0.5.0` | empty asset list | `tests.yml` |
| herdr-call | none in the first page | no release in the first page | `ci.yml` |
| herdr-agent-icons | none | no release | no workflow blob |
| herdr-plugin-examples | none on the first page | no release on the first page | no workflow blob |

Tag prefixes are mostly `v*`. `herdr-annotate` uses `rust-lite-v*`. Empty
asset lists mean the release object existed and its `assets` array was empty
in that response. They do not prove the tag has no source checkout.

**Inference:** checksum files are the shared habit of Rust and Go plugins that
download binaries at install time. Plugins whose `[[build]]` compiles from
source (`herdr-auto-title`, `herdr-call`, the cookbook Rust example) can ship
tags with no release binaries. Radar ships a `v*` tag and a changelog without
release assets because `[[build]]` runs `node bin/setup.js` from the checkout.

## Config and state

The plugins doc is the normative split: user-editable files in
`HERDR_PLUGIN_CONFIG_DIR`, runtime files in `HERDR_PLUGIN_STATE_DIR`, nothing
durable in `HERDR_PLUGIN_ROOT` because reinstall replaces that directory.

READMEs usually tell the user to run `herdr plugin config-dir <id>` rather
than naming the environment variable. Examples of files they place there:

- `herdr-agent-usage`: `config.toml`, created on first `usagebar.setup`.
- `herdr-radar`: `config.toml`, created when the settings popup saves.
- `herdr-file-viewer`: copy an example to `config.toml`.
- `herdr-plus`: a `projects/` directory of templates.
- `herdr-lazy`: `plugins.list` and `plugins.lock`.
- `herdr-icon-agent-ui`: the README shows `config-dir` as a verify command;
  the font install copies into the user font directory, which is outside the
  plugin config dir.

`herdr-lazy`'s manifest is the clearest state-dir example in this cohort: the
event command records context there and the comment says it does not mutate
the plugin list. Other state-dir uses were not confirmed by reading
implementation source. **Inference:** sidebar token publishers need state for
a daemon or a restored view, and pane-only tools can live without it. That is
consistent with radar's startup comments and with lineup's planned publisher,
but it was not proven by executing either plugin.

## Fonts and logos

Two different jobs show up.

Sidebar vendor glyphs are shipped as a font file inside the plugin tree:

- `hhdebb/herdr-radar` contains `dist/HerdrAgentIconsMax-Regular.ttf` and
  `dist/JetBrainsMonoHerdr-Regular.ttf`. The manifest has an `install-font`
  action whose title says it copies the icon font into the user font directory
  and maps codepoints. `tools/build_font.py` is in the tree. `THIRD_PARTY_NOTICES.md`
  is in the tree.
- `qintmb/herdr-icon-agent-ui` contains the same family name
  `dist/HerdrAgentIconsMax-Regular.ttf` plus `font/codepoints.toml` and
  `tools/build_font.py`. The README's install section says the font is required
  for icon graphics and shows `cp` into `~/Library/Fonts` on macOS.
- `BlockedPath/herdr-agent-icons` contains `fonts/CodexIcon.ttf` and a README
  section that tells the user to install that file and point the terminal at
  the glyph. Its manifest does not declare a build or a font-install action;
  startup is `bash agent-icons.sh`.

TUI plugins mention terminal fonts only as a client requirement, not as a
bundled sidebar font. `herdr-reviewr`'s README says box-drawing glyphs are
required and a Nerd Font is not. `herdr-sidebar`'s README says a Nerd Font is
recommended for material icons and that an emoji theme works without it.
`zoetrope` vendors `assets/fonts/JetBrainsMono-*.ttf` in the repository tree.
This survey did not check whether the Herdr plugin build installs those files
for the user's terminal. **Inference:** those files are for the graph renderer,
not for Herdr's sidebar codepoint map. The tree path is a fact; the install
behavior was not read.

Lineup's `NOTICE.md` already attributes Herdr Agent Icons Max to
`hhdebb/herdr-radar` and records `qintmb/herdr-icon-agent-ui` as a related
source. The design note in `openspec/changes/add-lineup-v1/design.md` says
this change copies no font binary. That earlier note matches the plugins
doc's ownership split: the terminal owns the font. It does not match radar
or icon-agent-ui, which install fonts for the user.

Decision 2026-10-12: follow `hhdebb/herdr-radar`. Lineup bundles the sidebar
icon font and adds opt-in `install-font` and `uninstall-font` actions. Startup
still must not rewrite font setup. The earlier "do not bundle a font" adoption
row is withdrawn. This document records the decision; another lane owns the
asset and the actions.

## herdr-lineup at this commit

Read from the worktree base `911a260e40e73bd7260089347a95cb2d041c6729`:

- `herdr-plugin.toml`: `id = "herdr-lineup"`, `version = "0.1.0"`,
  `min_herdr_version = "0.9.0"`, `platforms = ["macos", "linux"]`, one
  `[[build]]` of `cargo build --release --locked`, `[[startup]]` and
  `workspace.focused` both call `ensure`, and actions `start`, `stop`,
  `doctor`, `configure`, `unconfigure`. No panes, no link handlers, no
  per-item `platforms`.
- `README.md`: planned-versus-implemented is explicit. Install, usage,
  configuration, behavior, troubleshooting, development, and license are
  present. It already says `plugin link` does not build, names
  `$HERDR_PLUGIN_CONFIG_DIR` and `$HERDR_PLUGIN_STATE_DIR`, says startup will
  not edit user configuration, and says the plugin will not bundle or install
  fonts. It points at the plugins doc and says release tags use `v*`.
- `.github/workflows/ci.yml`: push, pull request, dispatch, and a weekly
  schedule. The check job runs on `ubuntu-latest` and `macos-latest` and runs
  `cargo fmt --all --check`, locked Clippy with `-D warnings`, locked tests,
  a locked release build, and `--help`. A separate audit job runs
  `cargo audit`. There is no release workflow in this commit.
- The repository is not in the marketplace index snapshot. The README calls
  public install planned, after publication. The formation goal for this work
  says the repository stays private. **Inference:** a private repository
  cannot meet the public-topic listing rule. That follows from the marketplace
  doc; this survey did not ask GitHub whether the topic is set.

## Differences and what to adopt

Priorities are for a later assignment. This document does not apply them.
"Keep" means the current file already matches the stronger convention and
should not be rewritten toward a peer that does the opposite.

| Priority | Adopt or keep | Target files | Why |
| --- | --- | --- | --- |
| P1 | The repository is now public; add the `herdr-plugin` topic after the README documents public installation. The existing required manifest fields are enough for the index parser. | `README.md` | Listing requires a public default branch plus the topic; publication alone does not list the plugin. |
| P1 | Ship `v*` tags whose version matches `Cargo.toml`, `Cargo.lock`, and `herdr-plugin.toml`, and attach `SHA256SUMS` for any downloaded binary. A cargo-only install does not need binary assets. | `.github/workflows/release.yml`, `scripts/check-version.sh`, `docs/releasing.md` | Checksums are the common contract of fetch-or-build plugins. Version drift showed up in `zoetrope` and `herdr-annotate`. Another lane owns the release workflow; do not duplicate it here. |
| P2 | Keep `cargo build --release --locked` as the install build. Add a download-and-verify script only after release assets exist. The cookbook example omits `--locked`; lineup is stricter. | `herdr-plugin.toml`, a future `scripts/install.sh` | Locked builds match `herdr-projects`' fallback and this repo's CI. A fetch script was optional while install was still planned at this commit. |
| P2 | Extend the README with three facts the plugins doc already states and several peers repeat: reinstall replaces `HERDR_PLUGIN_ROOT`; there is no `plugin update`; `herdr plugin install owner/repo --ref vX.Y.Z` pins a revision. Keep the existing planned-versus-implemented split and `config-dir` instructions. Font policy is the P3 decision dated 2026-10-12. | `README.md` | Those three facts are how users avoid storing state in the managed checkout. At this commit, Lineup's README did not say them yet. |
| P3 | Keep opt-in `configure` / `unconfigure` actions. Do not copy radar or icon-agent-ui startup that rewrites sidebar or font setup. | `herdr-plugin.toml`, `README.md` | Lineup already refuses startup edits. Peers that edit config from startup are a different product choice, not a marketplace requirement. |
| P3 | Bundle the sidebar icon font, following `hhdebb/herdr-radar`, and add opt-in `install-font` and `uninstall-font` actions. Keep `NOTICE.md` attribution. Do not install or remove the font from startup. | `herdr-plugin.toml`, `README.md`, `NOTICE.md` | Decision 2026-10-12: follow herdr-radar. The earlier row told lineup not to bundle `HerdrAgentIconsMax` and to leave the font to the user terminal. That row is withdrawn. The user opts in through `install-font` and opts out through `uninstall-font`. Startup still must not rewrite font setup. |
| P3 | Keep `id = "herdr-lineup"`. Dotted ids are common and legal. Change the id only if a public install collides. | `herdr-plugin.toml` | The plugins doc accepts both shapes. Renaming after install would orphan config directories. |
| P4 | Keep the current CI checks. Add `actionlint` when a release workflow lands. `cargo audit` is already a job; `herdr-reviewr` uses a separate `audit.yml`, which is optional. | `.github/workflows/ci.yml` | Lineup's check matrix is already stricter than plugins that only ship `release.yml`. |
| P4 | Do not add panes or link handlers to imitate review, file-viewer, or hunk-diff. Lineup publishes sidebar metadata. | `herdr-plugin.toml` | Those sections solve a different entrypoint. Copying them would invent UI the spec does not ask for. Leave this manifest without `[[panes]]` or `[[link_handlers]]`. |

Items marked P1 that touch release files are listed so the release lane can
see them. They are outside this assignment's touching path.
