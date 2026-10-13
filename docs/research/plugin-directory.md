# Herdr plugin directory

Checked 2026-10-13. Repository files are from `origin/main`
`47c181f18a8b876d38b3acbacdd828dfbfb3aab6`.

As of tag v0.1.1 (`cacef43efab070cd17e5a5f6448aabc5bdb039dc`, 2026-10-14), this repository is public and the README documents `herdr plugin install fujimogn/herdr-lineup`. The gap and owner notes below are that 2026-10-13 snapshot, including wording that was private or planned at `47c181f`.

## Conclusion

Listing is adding the GitHub topic `herdr-plugin` to this public
repository. `herdr-plugin.toml` is already on `main`. There is no
submission form, pull request, issue, or review queue. The index
refreshes every 30 minutes and when the default-branch head changes.

## How listing works

- The directory is <https://herdr.dev/plugins/>, documented at
  <https://herdr.dev/docs/marketplace/>.
- A public repository with the topic `herdr-plugin` is listed when
  its default branch has a parseable `herdr-plugin.toml` at the root
  or in a subdirectory.
  <https://herdr.dev/docs/marketplace/>
- One card per repository. Each valid manifest is a separate install.
  Forks, archived repositories, repositories without a valid
  manifest, and malformed manifest metadata are excluded.
  <https://herdr.dev/docs/marketplace/#what-a-listing-shows>
- The browse page says "No submission, no review queue."
  <https://herdr.dev/plugins/>
  <https://herdr.dev/docs/plugins/#trust-and-security>
- A card shows the GitHub name, owner, description, star count,
  primary language, and last push. Plugin rows show the manifest
  `name` and `version`.
  <https://herdr.dev/docs/marketplace/#what-a-listing-shows>
- Repository admins add topics. Topics appear on the repository page.
  <https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/classifying-your-repository-with-topics>
- GitHub topic page: <https://github.com/topics/herdr-plugin>.
  Index: <https://assets.herdr.dev/plugins/index.json>.

## Manifest and install

Required top-level fields are `id`, `name`, `version`, and
`min_herdr_version`. `description` is optional. Herdr refuses a
plugin whose minimum version is newer than the running binary.
<https://herdr.dev/docs/plugins/#manifest>

`GET /repos/fujimogn/herdr-lineup/contents/herdr-plugin.toml?ref=main`
decoded to the same opening fields as this checkout:

| Field | Location |
| --- | --- |
| `id = "herdr-lineup"` | `herdr-plugin.toml:1` |
| `name = "Lineup"` | `herdr-plugin.toml:2` |
| `version = "0.1.0"` | `herdr-plugin.toml:3` |
| `min_herdr_version = "0.9.3"` | `herdr-plugin.toml:6` |
| `description` | `herdr-plugin.toml:7` |
| `platforms = ["macos", "linux"]` | `herdr-plugin.toml:8` |
| `cargo build --release --locked` | `herdr-plugin.toml:10-11` |
| Rust `1.99.0` | `rust-toolchain.toml:2` |

`plugin install` accepts GitHub shorthand only, clones with `git`,
shows a preview in an interactive terminal, then runs build commands.
It does not install missing toolchains. A failed build is not
registered. `plugin link` does not build. There is no `plugin update`.
<https://herdr.dev/docs/plugins/#install-and-link>
<https://herdr.dev/docs/plugins/#build-commands>

```text
$ herdr --version
herdr 0.9.3
$ herdr plugin install --help
Usage: herdr plugin install [OPTIONS] <OWNER/REPO[/SUBDIR]>
      --ref <REF>
  -y, --yes
$ herdr plugin search --help
exit 2
commands: install, uninstall, link, unlink, enable, disable,
  list, config-dir, action, log, pane
```

## Gap against this repository

`GET /repos/fujimogn/herdr-lineup` returned:

| Check | REST value | Blocks listing |
| --- | --- | --- |
| Public | `visibility` `public`, `default_branch` `main` | No |
| Not a fork | `fork` `false` | No |
| Not archived | `archived` `false` | No |
| Topic `herdr-plugin` | `topics` `[]` | Yes |
| Required manifest fields | Present on `ref=main`, table above | No |

GitHub reports the license SPDX id `MIT` (`LICENSE:1-21`).

At `47c181f`, these lines said the repository was private or that public
installation was planned: `README.md:14-15`, `README.md:19`,
`README.md:48`, `README.md:94-103`, `docs/releasing.md:7-8`,
`CHANGELOG.md:25`. Finding 10 in
`docs/reviews/code-review-2026-10-13.md` left that planned wording
while the repository was private.

## Not checked

- <https://assets.herdr.dev/plugins/index.json> was not fetched again
  for `47c181f18a8b876d38b3acbacdd828dfbfb3aab6`.
- `herdr plugin install` was not run. No saved response shows an
  earlier `licenseInfo` of null; the REST read above is `MIT`.
- Web search described community link lists of the same topic. Those
  pages were not opened. The marketplace page does not name them.
- The published `v0.1.0` Release body, read earlier on 2026-10-13,
  included "public plugin installation remains planned." It was not
  fetched again for this commit.

## What the owner does

Outside the repository: add the topic `herdr-plugin` on
<https://github.com/fujimogn/herdr-lineup>, then wait for the
30-minute refresh or push a new `main` head.

Inside the repository, and not part of that listing procedure, the
private-repository sentences named above, including `CHANGELOG.md:25`
at that commit, were the follow-up. v0.1.1 replaced that install wording.
Editing a changelog does not replace a Release already published; the
v0.1.0 Release body was not fetched again for this note.
