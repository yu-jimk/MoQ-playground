# ③ webtransport/ — WebTransport (HTTP/3) 上の MoQ

ブラウザは生 QUIC を開けないが WebTransport は開ける。実際のライブ配信を
成立させるのはこのグループ。②（`moqt://`）との違いは**下に HTTP/3 が挟まる**点だけで、
その上に載る MoQ の pub/sub は同じ moq-net。

```text
  Browser(配信) ──publish live/<name>──┐
                                       ▼
                                  +---------+
                                  |  relay  |  ← 共有 origin で fan-out
                                  +---------+
                        ┌──────────┼──────────┬─────────────┐
                        ▼          ▼          ▼             ▼
                    Browser    Browser    Browser      moq-client (Rust)
                                (視聴 + チャット)
```

## 構成要素

| ディレクトリ | 役割 |
| --- | --- |
| [relay/](relay/) | WebTransport サーバ。全接続で 1 つの origin を共有し、publish された broadcast を視聴者へ流す |
| [client/](client/) | Rust クライアント（バイナリ `moq-client`）。チャットを web と相互運用する。映像はデコードしない |
| [web/](web/) | Vite + React。配信 / 視聴 / チャットを 1 画面で行う。詳細は [web/README.md](web/README.md) |

## 起動手順

**1. relay**（リポジトリのルートで）:

```bash
cargo run -p relay
```

- `https://[::]:4443` で待ち受ける（ALPN h3）。
- 自己署名証明書（ECDSA P-256 / 13 日）を生成し、その SHA-256 を標準出力と
  `webtransport/web/public/cert-hash.hex` に書き出す。**先に起動すること**。
- `RELAY_ADDR` / `RELAY_HASH_FILE` で上書きできる。

**2. web**:

```bash
cd webtransport/web
pnpm install   # 初回のみ
pnpm dev       # http://localhost:5173
```

**3. client**（任意）:

```bash
cargo run -p client -- alice   # 標準入力の各行をチャットとして送る
```

## 証明書のピン留め

ブラウザの WebTransport が `serverCertificateHashes` で自己署名証明書を受け入れる条件は
**ECDSA P-256 かつ有効期間 14 日以内**。relay はこれを満たす証明書を生成し、web は
`/cert-hash.hex` から読んだハッシュを渡して接続する。期限が切れたら relay を再起動すれば
新しい証明書とハッシュが生成される（ブラウザは再読み込み）。
