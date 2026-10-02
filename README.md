# MoQ（Media over QUIC）-playground

QUIC の生の API から、ブラウザで動くライブ配信までのテスト用リポジトリ。
下のレイヤから積み上がる3 グループに分けている。

```text
  ③ webtransport/   WebTransport (HTTP/3) 上の MoQ    ← ブラウザから配信 / 視聴 / チャットする
        ↑
  ② moq/            生 QUIC (moqt://) 上の MoQ        ← pub/sub の仕組みだけを見る
        ↑
  ① quic/           QUIC の stream と datagram        ← トランスポートそのもの
```

| ディレクトリ                   | 内容                                                | 使う技術                                              |
| ------------------------------ | --------------------------------------------------- | ----------------------------------------------------- |
| [quic/](quic/)                 | QUIC の双方向ストリーム / データグラムで ping-pong  | quinn, rustls, rcgen                                  |
| [moq/](moq/)                   | MoQ の publish / subscribe（生 QUIC 上）            | moq-net, web-transport-quinn                          |
| [webtransport/](webtransport/) | リレー + Rust クライアント + Web アプリでライブ配信 | web-transport-quinn (HTTP/3), moq-net, @moq/\*, React |

## ① quic/ — QUIC の stream と datagram

| クレート                          | 役割                                                                    |
| --------------------------------- | ----------------------------------------------------------------------- |
| [quic/transport](quic/transport/) | 自己署名証明書の生成と、検証をスキップするクライアント設定（①②③で共有） |
| [quic/stream](quic/stream/)       | 双方向ストリームで ping/pong                                            |
| [quic/datagram](quic/datagram/)   | データグラムで ping/pong                                                |

```bash
cargo run -p stream   --bin stream-server     # 別ターミナルで stream-client
cargo run -p datagram --bin datagram-server   # 別ターミナルで datagram-client
cargo test -p stream -p datagram              # ループバックの往復テスト
```

## ② moq/ — 生 QUIC 上の MoQ

`moqt://` は WebTransport を挟まず QUIC の上で直接 MoQ を話すスキーム。
MoQ のバージョンは ALPN でネゴシエートする。

```bash
cargo run -p moq --bin publisher    # broadcast を配信
cargo run -p moq --bin subscriber   # track を購読して frame を表示
```

## ③ webtransport/ — WebTransport (HTTP/3) 上の MoQ

ブラウザが話せるのは WebTransport なので、実際のライブ配信はこのグループ。

| 構成要素                                    | 役割                                                  |
| ------------------------------------------- | ----------------------------------------------------- |
| [webtransport/relay](webtransport/relay/)   | 共有 origin で fan-out する MoQ リレー（`[::]:4443`） |
| [webtransport/client](webtransport/client/) | Rust クライアント。web とチャットで相互運用する       |
| [webtransport/web](webtransport/web/)       | Vite + React の配信 / 視聴 / チャットアプリ           |

```bash
cargo run -p relay                                # ① まず relay（証明書ハッシュを書き出す）
cd webtransport/web && pnpm install && pnpm dev   # ② http://localhost:5173
cargo run -p client -- alice                      # ③ 任意。Rust から同じチャットへ
```

詳しい起動手順は [webtransport/README.md](webtransport/README.md) を参照。

## メモ

- Cargo ワークスペースは 1 つ。`target/` を共有するので `cargo build` はルートで一発。
- 証明書はすべて自己署名。ローカル検証専用で、実運用には使わない。
- relay の証明書は 13 日で失効する（ブラウザの上限が 14 日）。切れたら relay を再起動する。
