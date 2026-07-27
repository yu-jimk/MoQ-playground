# MoQ ライブ配信 — 統合 Web アプリ

1 つの Web アプリで配信 / 視聴 / チャットを行う。ブラウザから
WebTransport(HTTP/3)で `relay` クレートに接続し、MoQ で pub/sub する。

```text
   Browser(配信) ──publish live/<name>──┐
                                        ▼
                                   +---------+
                                   |  relay  |   ← webtransport/relay (WebTransport サーバ)
                                   +---------+
                          ┌───────────┼───────────┐
                          ▼           ▼           ▼
                      Browser      Browser      Browser  (視聴 + chat publish)
```

- **配信**: `<moq-publish>` が getUserMedia でカメラ/マイクを取得し、WebCodecs で
  H.264/Opus にエンコードして publish する。
- **視聴**: relay の announce で他の配信者を自動発見し、`<moq-watch>` を生やして再生する。
- **チャット**: `@moq/net` の track API でテキストframeを publish/subscribe する。
- **証明書**: relay は自己署名(ECDSA P-256 / 有効期間13日)。その SHA-256 を
  `/cert-hash.hex` から読み、WebTransport の `serverCertificateHashes` にピン留めする。

## 起動手順

**1. relay を起動**(リポジトリのルートで):

```bash
cargo run -p relay
```

- `https://[::]:4443` で待ち受ける。
- 証明書ハッシュを標準出力に表示し、`webtransport/web/public/cert-hash.hex` に
  書き出す。Web アプリはこのファイルを読む(**relay を先に起動すること**)。

**2. Web アプリを起動**(`webtransport/web/` で):

```bash
pnpm install   # 初回のみ
pnpm dev
```

- http://localhost:5173 を開く(localhost は http でも安全なコンテキスト扱い)。

**3. 使う**:

1. 名前を入れて **参加**(WebTransport 接続 + チャット購読開始)。
2. **配信開始**でカメラ/マイクを許可 → 自分の映像が publish される。
3. 別タブ/別端末で同じ手順を踏むと、互いの映像が自動でタイル表示され、
   チャットも流れる。視聴だけなら「配信開始」を押さなければよい。

> 証明書は 13 日で失効する。切れたら relay を再起動すれば新しい証明書と
> ハッシュが生成される(ブラウザは再読み込み)。

## 構成 (React + Vite)

| ファイル                        | 役割                                                                |
| ------------------------------- | ------------------------------------------------------------------- |
| `index.html`                    | `#root` のみ。エントリは `src/main.tsx`                             |
| `src/main.tsx`                  | React ルートのマウント                                              |
| `src/App.tsx`                   | 画面全体(参加/配信/視聴グリッド/チャット)と状態管理                 |
| `src/moq.ts`                    | MoQ 接続の純ロジック(証明書ハッシュ・接続・チャット・announce 発見) |
| `src/components/MoqPublish.tsx` | `<moq-publish>` ラッパ(ref で証明書ハッシュ注入)                    |
| `src/components/MoqWatch.tsx`   | `<moq-watch>` ラッパ(同上)                                          |
| `src/custom-elements.d.ts`      | カスタム要素の JSX 型宣言                                           |
| `src/styles.css`                | スタイル                                                            |
| `vite.config.ts`                | React プラグイン + dev サーバ(5173)                                 |
| `public/cert-hash.hex`          | relay が書き出す証明書ハッシュ(git 管理外)                          |

## 依存

- React 19 + Vite 6(`@vitejs/plugin-react`)
- `@moq/net` — MoQ ネットワーク層(接続・pub/sub)
- `@moq/publish` / `@moq/watch` — 配信/視聴の Web Components(内部で `@moq/hang`)

## Rust クライアント(相互運用)

[`webtransport/client`](../client/)(バイナリ `moq-client`)は同じ relay に
WebTransport で繋ぎ、この Web アプリと**チャットで相互運用**できる
(メディアはデコードせず存在のみ表示)。

```bash
cargo run -p relay             # 先に relay を起動
cargo run -p client -- alice   # 別ターミナル。標準入力の各行をチャット送信
```

ブラウザと違い証明書ハッシュのピン留めは不要で、証明書の検証自体をスキップする
(ローカル検証専用)。接続先とユーザ名は `RELAY_URL` / `RELAY_NAME` で上書きできる。
