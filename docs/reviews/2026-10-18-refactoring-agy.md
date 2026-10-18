# herdr-lineup リファクタリング調査報告書 (2026-10-18)

- **調査担当**: lane_agy_refactor (Antigravity / agy)
- **対象コミット**: `f678ad7` (origin/main, Kimi 週次配線完了時点)
- **調査スコープ**: リポジトリ全体の設計・ドメインモデル・責務境界・コード品質の調査と改善提案

---

## 1. 調査の目的と前提

### 1.1 目的
`herdr-lineup` は、ターミナル／セッションマネージャ「Herdr」のサイドバーに、AI コーディングエージェントの状態・コンテキスト使用率・週次クォータ残量、およびリポジトリごとの最新出荷コミット時刻（ship time）を統合表示する常駐デーモン兼プラグインである。
直近で Kimi 週次クォータの配線（`f678ad7`）が完了した段階において、今後の機能拡張や保守を安全かつ容易に行えるよう、リポジトリ全体を以下の観点から調査・評価し、具現的なリファクタリング計画を提示する。

### 1.2 評価観点
1. **ドメインモデル・業務ルールの整合性**: プロジェクト固有の概念（agent, pane, session, context, weekly quota, ship time, token, provider）がシンプルかつ拡張しやすくモデル化されているか。
2. **設計資料と実装の整合性**: OpenSpec（`openspec/specs/`）や README と実装の間で、用語・責務・境界が整合しているか。
3. **重要なルール・不変条件の集約**: 鮮度（freshness）、オプトイン境界、unavailable の扱い、パーセント値の範囲（[0.0, 100.0]）が適切な型・モジュールに集約されているか。
4. **カプセル化と内部状態の整合性**: 内部状態と不変条件が適切に保護されているか。
5. **関心の分離（SoC）**: 中核ロジック、I/O、表示、外部連携が分離されているか。
6. **重複コードの排除**: reader 間、quota worker 間、キャッシュ機構等で同じ意味の処理が重複していないか。
7. **設定・定数の集約**: 設定や定数が適切に配置されているか。
8. **コード構造・複雑度**: 長大な関数（`src/publish.rs` など）、深いネスト、過剰な結合、循環依存がないか。

### 1.3 調査の制約と前提
- コード変更や実装は行わず、調査・レビュー・改善提案に限定する。
- DB・Web API サーバー・UI フレームワークは存在せず、今後も導入しない前提で評価する（CLI と常駐デーモンの用途に特化）。
- 本番の個人設定・認証情報・実セッション・外部ネットワーク・実 Herdr サーバーには一切触れず、コード静的解析および隔離されたテスト環境からの推定・実測に基づいて評価する。
- 形式的な DDD や過剰な抽象化、将来の仮定のみに基づく仕組みは提案しない。

---

## 2. システム概要と主要処理経路の把握

### 2.1 全体構成
本クレート（単一 Rust クレート）は以下の主要モジュール群で構成される：
- **CLI / コマンドディスパッチ (`src/lib.rs`, `src/main.rs`)**: `ensure`, `daemon`, `stop`, `doctor`, `configure`, `unconfigure`, `install-font`, `uninstall-font`, `statusline` の受付とルーティング。
- **デーモン・プロセスライフサイクル (`src/daemon.rs`, `src/lock.rs`, `src/socket.rs`)**: セッションごとの排他ロック獲得（`flock`）、ソケット inode ピン留め検証、子プロセスフォーク、`control.sock` による安全な終了ハンドリング。
- **パブリッシャー・メタデータ計画 (`src/publish.rs`, `src/tokens.rs`)**: Herdr ソケットとの JSON-RPC 通信、スナップショット取得、差分メタデータ計画（シーケンス管理・TTL 管理・容量制限下での間引き）、永続化チェックポイント（`sequences.json`）。
- **ローカルセッションリーダー (`src/reader_*.rs`, `src/statusline.rs`, `src/model.rs`)**: 各エージェント（Claude, Codex, Grok, Antigravity, Kimi, Hermes）のローカルログ／セッションファイル／statusline 記録からのコンテキスト使用量抽出。
- **アカウント週次クォータワーカー (`src/quota_*.rs`, `src/model.rs`)**: ネットワークオプトイン時に動作する独立ポーリングワーカー（Codex app-server RPC、Grok HTTP REST、Kimi HTTP REST）。
- **ユーザー設定・サイドバー・フォント・診断 (`src/config.rs`, `src/configure.rs`, `src/font.rs`, `src/doctor.rs`, `src/ship_time.rs`)**: TOML 設定パース、Herdr config のアトミック編集、フォント・端末マップ設定、ヘルスチェック、Git 出荷時刻抽出。

### 2.2 主要な処理経路（Execution Paths）
1. **デーモン初期化・起動 (`daemon::ensure` / `daemon::foreground`)**:
   - `HERDR_SOCKET_PATH` のデバイス・inode を検証し、セッション排他ロックを取得。
   - `MetadataPublisher` を構築。オプトイン設定（`allow_network`, `kimi_weekly_enabled`）に基づき、各週次クォータワーカー（`WeeklyWorker`）をバックグラウンドスレッドで起動。
2. **リフレッシュループ (`MetadataPublisher::refresh`)**:
   - 周期（デフォルト 5,000ms）ごとに Herdr UNIX ソケットへ新規接続し、`session.snapshot` を取得。
   - 消失したペイン／ワークスペースを検知し、チェックポイントのシーケンス床（`endpoint_floors`）を更新して削除（`reconcile_missing`）。
   - 各ワークスペースについて、Git リポジトリ identity と最新ローカル `refs/remotes/origin/main` コミット日時（`ship_time`）を解決。
   - 各ペインについて、担当エージェントを判定し、対応する `UsageReader` を実行（5秒周期に対して `reader_timeout_ms` の共有バジェット内で安全にタイムアウト制御）。
   - 有効な週次クォータ（メモリキャッシュ）を照合・オーバーレイ。
   - `tokens::pane_tokens` により 14 種類のペインメタデータを算出（第1ペイン判定・ロゴインデント付与・容量優先度 `PANE_CAPACITY_PRIORITY` による間引き）。
   - チェックポイントをアトミックに `fsync` 永続化した後、Herdr へ `report_metadata` を送信。
3. **statusLine 記録経路 (`statusline::run`)**:
   - Claude または Antigravity の statusLine フックから標準入力（JSON）を受信。
   - コンテキスト率・週次残量率・リセット時刻のみを抽出し、`$HERDR_PLUGIN_STATE_DIR/statusline/` 配下にセッション／ペイン ID を安全にハッシュエンコードしたファイル名でアトミック保存。
   - `--then` 指定時は元の標準入力バイト列をそのままシェルコマンドへストリーム中継。

---

## 3. ドメインモデルと主要概念の整理

本プロジェクトには明文化された独立のドメインモデル定義書が存在しないため、コードおよび OpenSpec 仕様書から抽出・整理した主要概念を以下に明記する（推定を含む）。

```
+-----------------------------------------------------------------------------------+
|                                 Herdr Session                                     |
|  +-----------------------------------+   +-------------------------------------+  |
|  |             Workspace             |   |                Pane                 |  |
|  | - workspace_id                    |   | - pane_id                           |  |
|  | - cwd / checkout_path             |   | - workspace_id                      |  |
|  | - Repository (name, ship_unix)    |   | - cwd                               |  |
|  +-----------------------------------+   | - Occupant (agent, native_session)  |  |
|                                          | - State (idle/working/blocked/...)  |  |
|                                          +-------------------------------------+  |
+-----------------------------------------------------------------------------------+
                                                      |
                                                      v
                                           +---------------------+
                                           |    UsageReading     |
                                           | - context (pct)     |
                                           | - weekly (pct, reset|
                                           | - observed_at       |
                                           +---------------------+
                                            /                   \
                                           v                     v
                             [Local Reader]              [Weekly Worker]
                             - CodexRollout              - Codex app-server
                             - GrokSignals               - Grok REST
                             - KimiSession               - Kimi REST
                             - StatusLine (Claude/Agy)
```

### 主要ドメイン概念の定義
- **Agent / Provider**: サポート対象の AI コーディングエージェント（Claude, Codex, Grok, Antigravity, Kimi, Hermes）。表示層では `Agent`、リーダー層では `Provider` と呼称されている。
- **Pane / Occupant**: Herdr 上の 1 画面分割単位（Pane）。そこにアタッチされているエージェント種別と固有セッション ID の組を `Occupant` と呼ぶ。
- **Context Usage**: 会話コンテキストウィンドウの消費率（0.0%〜100.0%）。レート制限ではなく、現在のセッションに紐づくローカルログやフックから抽出される。
- **Weekly Quota**: 7 日間（または特定週）の残りクォータ割合（0.0%〜100.0%）。原則オプトインのネットワーク通信によってアカウント単位で取得され、ペイン間で共有される。
- **Ship Time**: 各ワークスペースのローカル `refs/remotes/origin/main` の最新コミット時刻。リモート通信は行わず、手元で把握している出荷時刻を表示する。
- **Metadata Patch / Sequence**: Herdr に送信する表示用トークン群（`lineup_*`）。同一リソースへの更新が順序逆転しないよう、リソースごとの単調増加シーケンス番号（`seq`）と生存期間（`ttl_ms`）を持つ。

---

## 4. リファクタリング指摘一覧

### 指摘 1: `src/publish.rs` の神オブジェクト（God Object）化と長大関数 `refresh_with_readers` の責務過多

- **対象箇所**:
  - `src/publish.rs`: `MetadataPublisher` 構造体定義および `impl` (lines 231–1145)
  - `src/publish.rs`: `refresh_with_readers` 関数 (lines 728–1084, 計 356 行)
  - `src/publish.rs`: `start_weekly` 関数 (lines 330–485, 計 155 行)
- **問題と発生条件**:
  - `MetadataPublisher` が 20 個以上のフィールドを抱え、ソケット通信、チェックポイントファイル入出力、3 種類のバックグラウンドワーカー起動・排他制御、診断結果ファイル書き込み、Git 出荷時刻キャッシュ、ローカルセッション読み取り結果キャッシュ、ペイン／ワークスペースのトークン算出、キャパシティ超過時のキー間引きをすべて単一の構造体内で処理している。
  - `refresh_with_readers` は 356 行に及ぶ長大な関数であり、ループ内部に多数の条件分岐・ネスト・クロージャが含まれ、可読性と追跡性が著しく低下している。
- **根拠と影響**:
  - 単一責任の原則（SRP）に明白に違反している。例えば「週次クォータのキャッシュ制御」を変更する際に、まったく無関係な「ソケット通信」や「Git ship_time 算出」と同じ巨大ファイルを編集する必要がある。
  - `src/publish.rs` はテストコードを含めて 4,331 行（178KB）に達しており、単一ファイル内に全結合が集約されているため、テストのモック作成や部分検証が困難になっている（テストコードだけで 3,130 行以上を占める）。
- **改善案**:
  - `MetadataPublisher` を以下の独立したコンポーネントに責務分割する：
    1. **`QuotaManager`**: 3 つの `WeeklyWorker` の起動・停止・最新サンプル取得・診断記録をカプセル化。
    2. **`WorkspacePublisher` (または `WorkspaceReconciler`)**: ワークスペースの Git リポジトリ情報解決、ヘッディングキャッシュ管理、トークン生成を担当。
    3. **`PanePublisher` (または `PaneReconciler`)**: ペインごとの reader 呼び出し、キャッシュ照合、quota オーバーレイ、トークン生成を担当。
    4. **`MetadataPublisher`**: ソケット通信、スナップショット取得、差分レポート送信（`planner.prepare` / `report`）のオーケストレーションに専念。
- **優先度・想定工数・リスク**:
  - **優先度**: High
  - **想定工数**: 2〜3 人日
  - **リスク**: 中（既存テストが充実しているためリグレッションの検知は容易だが、内部キャッシュや状態の受け渡しインターフェースの慎重な設計が必要）。
- **改善後の検証方法**:
  - `cargo test --locked`（特に `publish::tests` および `tests/resource-lifecycle.rs`）がすべて成功すること。
  - `MetadataPublisher` の行数がテスト込みで 1,500 行以下に縮小し、各サブモジュールが独立して単体テスト可能になること。

---

### 指摘 2: プロバイダ・エージェント概念の型分裂と手動マッピングの散在

- **対象箇所**:
  - `src/model.rs:10–19`: `pub enum Provider { Claude, Agy, Codex, Grok, Hermes, Kimi }`
  - `src/tokens.rs:44–52`: `pub enum Agent { Claude, Codex, Grok, Agy, Kimi, Hermes }`
  - `src/statusline.rs:14–19`: `pub enum Agent { Claude, Agy }`
  - `src/doctor.rs:55–60`: `pub(crate) enum WeeklyProvider { Codex, Grok, Kimi }`
  - `src/config.rs:15`: `pub providers: Vec<String>`
  - `src/publish.rs:750–757`, `src/publish.rs:871–894`
- **問題と発生条件**:
  - 「エージェント／プロバイダ」という同一のドメイン概念に対して、4 つの異なる enum と文字列ベクタ（`Vec<String>`）が個別に定義されている。
  - `src/publish.rs` において、`reading.identity.provider` と `who.agent` の整合性を検証するために、以下のような手動タプルマッチングが行われている：
    ```rust
    matches!(
        (reading.identity.provider, who.agent),
        (model::Provider::Claude, Agent::Claude)
            | (model::Provider::Codex, Agent::Codex)
            | (model::Provider::Grok, Agent::Grok)
            | (model::Provider::Agy, Agent::Agy)
            | (model::Provider::Kimi, Agent::Kimi)
    )
    ```
  - `config.providers` は `String` のベクタであるため、各所で `.any(|p| p == "kimi")` や `Agent::from_id(p)` といった文字列比較が散在している。
- **根拠と影響**:
  - 新しいプロバイダを追加する際（あるいは既存の動作を変更する際）、4 つの enum と各所のマッチ文、文字列比較をすべて手作業で追従させる必要があり、コンパイラによる網羅性チェック（exhaustive check）の恩恵を十分に受けられない。
- **改善案**:
  - コアドメイン層（`src/model.rs` または新規 `src/agent.rs`）に、統合された `AgentId`（または `Provider`）enum を定義する。
  - 表示メタデータ（id, label, glyph）、ケーパビリティ（ローカルリーダーの有無、週次クォータの有無、statusLine 対応の有無）をこの enum のメソッドとして集約する。
  - `tokens::Agent`, `statusline::Agent`, `doctor::WeeklyProvider` を統合または相互変換可能にし、`Config.providers` も `Vec<AgentId>` としてデシリアライズ時に検証・保持する。
- **優先度・想定工数・リスク**:
  - **優先度**: Medium
  - **想定工数**: 1〜1.5 人日
  - **リスク**: 低（純粋な内部型の統一であり、外部通信や Herdr プロトコルの JSON 互換性は容易に維持可能）。
- **改善後の検証方法**:
  - `Config::parse` のバリデーションテストおよび `tokens::tests` の挙動が変わらないこと。

---

### 指摘 3: Quota Worker における構造・設定・鮮度判定ロジックの重複（Grok と Kimi の重複）

- **対象箇所**:
  - `src/quota_grok.rs:17–44` と `src/quota_kimi.rs:17–43` (`Settings` 構造体、clamp ロジック、`default()`)
  - `src/quota_grok.rs:75–79` と `src/quota_kimi.rs:80–84` (`WeeklyUsage` 構造体)
  - `src/quota_codex.rs:24–29` (`Weekly` 構造体)
  - `src/model.rs:114–118` (`WeeklySample` 構造体)
  - `src/publish.rs:123–138` (`fresh_grok_weekly`) と `src/publish.rs:158–173` (`fresh_kimi_weekly`)
  - `src/quota_grok.rs:47–73` と `src/quota_kimi.rs:46–78` (`Failure` enum および `Failure::diagnostic()`)
- **問題と発生条件**:
  - `quota_grok.rs` と `quota_kimi.rs` で、`Settings` 構造体の定義、clamp 処理（60..=3600秒、1..=30秒）、デフォルト値（300秒、10秒）が完全に同一のコードとして重複している。
  - クォータ残量データを表す構造体（`used_percent: f64`, `resets_at: Option<SystemTime>`）が、`model::WeeklySample`, `quota_codex::Weekly`, `quota_grok::WeeklyUsage`, `quota_kimi::WeeklyUsage` の 4 つに分裂しており、`publish.rs` で毎回フィールドの詰め替えが行われている。
  - リセット時刻がない場合の週次クォータ有効期限計算ルール（`Duration::from_millis(max_age_ms).max(query_cycle)`）が `fresh_grok_weekly` と `fresh_kimi_weekly` で 100% 同一のコードとしてコピー＆ペーストされている。
- **根拠と影響**:
  - 仕様上重要な業務ルール（「リセット時刻のない週次クォータは、ポーリング間隔＋タイムアウト＋リフレッシュ間隔と max_age の大きい方で期限切れとする」）が型やモジュールにカプセル化されず、関数として重複定義されている。
  - 将来ポーリング設定の境界値や鮮度ポリシーを変更する際、複数箇所を同期して修正する必要があり、修正漏れのリスクがある。
- **改善案**:
  - `model::WeeklySample`（または `WeeklyQuota`）に統一し、各クライアントから直接この型を返却させる。
  - HTTP クォータポーリング設定 `QuotaPollingSettings` を共通化する。
  - 鮮度判定ロジックを `WeeklySample` のメソッド（`WeeklySample::is_fresh(&self, observed, now, max_age, refresh_interval, settings)`）として集約する。
- **優先度・想定工数・リスク**:
  - **優先度**: High
  - **想定工数**: 1 人日
  - **リスク**: 低（API 通信部や固有のトークン解決ロジックには手を触れず、戻り値と共通ヘルパーのみを統合するため安全）。
- **改善後の検証方法**:
  - `cargo test --locked` で Grok / Kimi / Codex のポーリングおよびキャッシュテストが全件パスすること。

---

### 指摘 4: 生の `f64` によるパーセント値表現と不変条件の型安全性欠如（Primitive Obsession）

- **対象箇所**:
  - `src/model.rs:50, 116, 349`: `percent.is_finite() && (0.0..=100.0).contains(&percent)`
  - `src/tokens.rs:132, 133, 181, 182, 204–206`: `fn percentage(value: Option<f64>) -> Option<f64>`
  - `src/statusline.rs:30, 31, 53–56`: `(value.is_finite() && (0.0..=100.0).contains(&value)).then_some(value)`
  - `src/reader_grok.rs:125`: `used < 0.0 || capacity <= 0.0`
  - `src/reader_codex.rs:35`
  - `src/reader_kimi.rs:104`
- **問題と発生条件**:
  - コンテキスト使用率および週次残量クォータのパーセンテージが、システム全体を通して生のプリミティブ型 `f64` で受け渡されている。
  - 仕様書（`spec.md:39-40`）では「非有限値や範囲外（[0, 100] 外）は clamp せず unavailable とする」と厳格に規定されているにもかかわらず、各所で個別に `is_finite() && (0.0..=100.0).contains(...)` のガードコードが書かれている。
- **根拠と影響**:
  - 基本型への執着（Primitive Obsession）により、どの関数・型が「検証済みのパーセント値」を保証しているかが型システム上で表現されていない。
  - 将来の計算処理やリファクタリング時に、チェック漏れによる NaN や範囲外数値の混入をコンパイル時に防止できない。
- **改善案**:
  - 値オブジェクト `Percentage`（Newtype パターン: `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Percentage(f64);`）を導入する。
  - コンストラクタ `Percentage::new(value: f64) -> Result<Self, MissingReason>`（または `Option<Self>`）でのみインスタンス化可能にし、不変条件（`value.is_finite() && (0.0..=100.0).contains(&value)`）を強制する。
  - 四捨五入・表示形式生成（`format!("{:02}%", self.0.round() as u8)`）も本型に持たせる。
- **優先度・想定工数・リスク**:
  - **優先度**: Medium
  - **想定工数**: 1 人日
  - **リスク**: 低（純粋な内部型の導入であり、外部I/O境界でのみ `f64` と相互変換する）。
- **改善後の検証方法**:
  - 不正な値（NaN, Infinity, -0.1, 100.1）を渡す既存の単体テストがすべて期待通り `Unavailable(MissingReason::Invalid)` となること。

---

### 指摘 5: Reader（Codex / Kimi）における LRU 風キャッシュとアペンド検知の重複

- **対象箇所**:
  - `src/reader_codex.rs:10–20, 86–96, 126–137`
  - `src/reader_kimi.rs:17–26, 109–119, 120–132`
- **問題と発生条件**:
  - `reader_codex` と `reader_kimi` の両方で、`LazyLock<Mutex<HashMap<(PathBuf, String), ...>>>` を用いた最大 128 件のインメモリキャッシュが個別に定義されている。
  - キャッシュ上限に達した際に `cache.clear()` で全削除する簡易的な運用を含め、構造が同一である。
  - ファイルの `Fingerprint`（dev, inode, size, mtime）を取得し、読み取り前・読み取り後・ファイル stat の 3 点でアペンド判定（`snapshot_change`）を行ってキャッシュを更新するフローが重複している。
- **根拠と影響**:
  - 同一パターンのキャッシュおよび並行書き込み追跡ロジックが別々のファイルに独立して書かれており、バグ修正やキャッシュ戦略改善（例: 全削除ではなく LRU 追い出し）を行う際に二重メンテナンスが発生する。
- **改善案**:
  - `src/model.rs` に、ファイルベースのセッション読み取り専用キャッシュ `SessionFingerprintCache<T>` を導入し、Fingerprint の照合・キャッシュ容量管理・アペンド検証を共通コンポーネント化する。
- **優先度・想定工数・リスク**:
  - **優先度**: Low〜Medium
  - **想定工数**: 0.5〜1 人日
  - **リスク**: 低。
- **改善後の検証方法**:
  - `reader_codex::tests` および `reader_kimi::tests` のキャッシュヒット、アペンド検知、ファイル置換検知テストが正常に動作すること。

---

### 指摘 6: `Config` の型表現の弱さと仕様書（`daemon-lifecycle/spec.md`）との軽微な乖離

- **対象箇所**:
  - `src/config.rs:13`: `pub logo_mode: String`
  - `src/config.rs:15`: `pub providers: Vec<String>`
  - `openspec/specs/daemon-lifecycle/spec.md:109–118` (Config 表)
- **問題と発生条件**:
  - `Config.logo_mode` は `"auto"`, `"font"`, `"text"` の 3 値しか許容されないが、構造体上は `String` 型として定義され、手動で `validate()` 内でチェックしている。さらに `publish.rs:799–803` で `tokens::LogoMode` へ変換している。
  - `openspec/specs/daemon-lifecycle/spec.md` の第 109–118 行目の設定契約表に、直近の機能追加である `kimi_weekly_enabled` が反映されておらず、仕様書と実装で契約一覧が不一致になっている。
- **根拠と影響**:
  - serde のデシリアライズ機能により `logo_mode` を enum として直接パースすれば、バリデーションコードと再マッチングコードを排除できる。
  - 仕様書の表が古いままだと、設定項目の仕様を確認する開発者が `kimi_weekly_enabled` を見落とす原因となる。
- **改善案**:
  - `tokens::LogoMode` に serde の `Deserialize` を実装し、`Config.logo_mode: LogoMode` とする。
  - `openspec/specs/daemon-lifecycle/spec.md` の表に `kimi_weekly_enabled`（default: `false`, contract: `Boolean; opt-in credential-read for Kimi weekly quota`）を追記する。
- **優先度・想定工数・リスク**:
  - **優先度**: Low
  - **想定工数**: 0.5 人日
  - **リスク**: 極低。
- **改善後の検証方法**:
  - `config::tests` の正常系・異常系テストがパスすること。

---

### 指摘 7: `MetadataPublisher` と `doctor` 診断ファイル書き込みの強結合

- **対象箇所**:
  - `src/publish.rs:331, 351, 380, 389, 416, 480`
  - `src/doctor.rs:74–86`: `write_weekly_result`
- **問題と発生条件**:
  - `MetadataPublisher` の `start_weekly` メソッド内部で、クォータワーカーの各ポーリング結果を `doctor::write_weekly_result` を直接呼び出して `$HERDR_PLUGIN_STATE_DIR/weekly-{provider}.json` に書き出している。
- **根拠と影響**:
  - パブリッシャーは「Herdr へのメタデータ配信」が主責務であるにもかかわらず、CLI の `doctor` コマンド向け診断ファイル管理という別ドメインの責務を直接抱え込んでいる（関心の分離違反）。
  - パブリッシャーと診断モジュールが密結合しており、モジュール境界が不鮮明になっている。
- **改善案**:
  - 診断結果の記録責務を、パブリッシャー本体ではなくクォータワーカー（または前述の `QuotaManager`）内部に閉じ込めるか、コールバック／イベント通知形式に変更する。
- **優先度・想定工数・リスク**:
  - **優先度**: Medium
  - **想定工数**: 0.5 人日
  - **リスク**: 低。
- **改善後の検証方法**:
  - `doctor::tests` で各プロバイダの診断結果（ok, pending, disabled, auth_missing 等）が正しく表示されること。

---

## 5. 見送った観点（過剰な抽象化の防止）

本調査にあたり、検討した上で意図的に見送った観点は以下の通りである：

1. **データベース（SQLite 等）や Web サーバーフレームワークの導入**:
   - 本プラグインはローカルで常駐する軽量デーモンであり、状態はメモリキャッシュと小さな JSON チェックポイント（`sequences.json`）で完結している。永続化ライブラリの導入はオーバーヘッドと依存関係を増大させるため見送った。
2. **非同期ランタイム（Tokio 等）への全面移行**:
   - 現在のアーキテクチャは同期ブロッキング I/O、スレッド分離（`WeeklyWorker`, `bounded_read`）、および `poll` によるデッドライン監視を採用しており、堅牢に動作している。Tokio への移行はバイナリサイズ増加、プラットフォーム互換性（macOS/Linux のソケット終了エッジケース）の再検証コストが見合わないため見送った。
3. **形式的なドメイン駆動設計（DDD）による過剰な多層化**:
   - 単一目的の CLI/デーモンに対して、Repository パターン、Entity/ValueObject の厳密な階層化、UseCase サービス層の導入を行うと、単にコードのボイラープレートが増加するだけとなる。Rust のモジュールシステムと型システム（Newtype, enum）を自然に活かしたリファクタリングに留める。
4. **Hermes リーダーの本格実装**:
   - Hermes は現在公式なローカル利用データソースが確立されておらず、OpenSpec 上も「Stage 1 では Unavailable」と定義されている。仮定に基づいた推測実装は行わず、現状のスタブのままとする。

---

## 6. 未検証事項と制約

- **実測・コードからの推定・未検証の区別**:
  - **実測**:
    - コードベース全体の行数・複雑度集計（`scc` により全体 18,860 行、`src/publish.rs` が 4,330 行であることを確認）。
    - Git コミット履歴・リビジョン・ブランチ構成の確認。
  - **コードからの推定**:
    - `publish.rs` のコンポーネント分割時における結合度低下効果とリファクタリング工数。
    - `Percentage` 値オブジェクト導入による境界値バグ抑止効果。
  - **未検証**:
    - macOS 実機における `control.sock` パス長境界値（104 バイト制限）の動作（Linux 上での調査のため）。
    - 本物の Kimi / Codex / Grok アカウントによる実ネットワーク通信（セキュリティ規約および指示に基づき、実クレデンシャルや外部 API 通信は一切遮断）。

---

## 7. まとめと推奨実施ステップ

### 改善案の優先度マトリクス

| 指摘 | 項目 | 優先度 | 想定工数 | 主な効果 |
| :--- | :--- | :---: | :---: | :--- |
| **指摘 1** | `src/publish.rs` の神オブジェクト分割 (`QuotaManager`, `PanePublisher` 等) | **High** | 2〜3 人日 | 巨大ファイル（4,330行）の解体、保守性・可読性・テスト容易性の劇的向上 |
| **指摘 3** | Quota Worker（Grok / Kimi / Codex）の型・鮮度判定ロジック統合 | **High** | 1 人日 | コピペコード排除、重要な有効期限計算ルールの集約 |
| **指摘 2** | プロバイダ・エージェント概念の統合 (`AgentId` enum) | **Medium** | 1〜1.5 人日 | 4 箇所に分裂した enum の統一、手動マッピング排除 |
| **指摘 4** | `Percentage` 値オブジェクト導入による型安全性強化 | **Medium** | 1 人日 | 生 `f64` 排除、不変条件 [0.0, 100.0] のコンパイル時強制 |
| **指摘 7** | `MetadataPublisher` と `doctor` 診断ファイル書き込みの疎結合化 | **Medium** | 0.5 人日 | 関心の分離、パブリッシャーの責務純化 |
| **指摘 5** | Reader（Codex / Kimi）のキャッシュ・アペンド検知共通化 | **Low〜Med** | 0.5〜1 人日 | 重複した LRU 風キャッシュと Fingerprint 照合の統一 |
| **指摘 6** | `Config` の型表現改善と OpenSpec ドキュメント整合 | **Low** | 0.5 人日 | `logo_mode` の enum 化、仕様書の追従 |

### 推奨実施ステップ
1. **フェーズ 1: ドメイン型・業務ルールの集約（指摘 2, 3, 4）**
   - `Percentage` 値オブジェクト、`WeeklySample` / 鮮度判定ルールの統合、`AgentId` への統一。これらは純粋ロジック層の変更であり、リスクを低く抑えながら基礎を固められる。
2. **フェーズ 2: QuotaManager と診断の切り出し（指摘 3, 7）**
   - `src/publish.rs` からワーカー管理と診断記録を `src/quota_manager.rs` に分離。
3. **フェーズ 3: `MetadataPublisher` の構造改革（指摘 1, 5, 6）**
   - `refresh_with_readers` を `WorkspacePublisher` と `PanePublisher` に分解し、オーケストレーターとして再構成する。
