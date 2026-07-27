# ② moq/ — 生 QUIC 上の MoQ

MoQ の pub/sub だけを見るための段。ブラウザは相手にしないので WebTransport
（HTTP/3）を挟まず、`moqt://` = **生 QUIC の上で直接 MoQ を話す**。
③ [webtransport/](../webtransport/) との違いは下に HTTP/3 が入るかどうかだけで、
その上に載る moq-net の使い方は同じ。

```text
  publisher ──broadcast/track/frame──▶ subscriber
       └─────── moq-net ───────┘
       └─────── QUIC (ALPN: moqt-19) ───────┘
```

## 仕組み

- **バージョン = ALPN**。生 QUIC ではハンドシェイクの ALPN で MoQ のバージョンを
  ネゴシエートする（`MOQ_VERSION = "moq-transport-19"` → ALPN は `moqt-19`）。
- moq-net の `Client` / `Server` は `web_transport_trait::Session` を要求するので、
  確立した quinn 接続を `web_transport_quinn::Session::raw` で WebTransport
  セッションに"見せかけて"渡す（moq-native がやっていることを手で組む）。
- QUIC / TLS の下ごしらえは [quic/transport](../quic/transport/) を再利用する。

| ファイル | 役割 |
| --- | --- |
| [src/lib.rs](src/lib.rs) | エンドポイント生成（client / server）と接続 / 受理のヘルパ |
| [src/bin/publisher.rs](src/bin/publisher.rs) | サーバを立てて broadcast を配信する |
| [src/bin/subscriber.rs](src/bin/subscriber.rs) | 接続して track を購読し、届いた frame を表示する |

## 実行

```bash
cargo run -p moq --bin publisher     # [::]:4443 で待ち受ける
cargo run -p moq --bin subscriber    # 別ターミナル。moqt://localhost:4443 へ
```

`localhost` は `::1` に解決されがちなので、publisher はデュアルスタックの `[::]`
で待ち受け、subscriber は自分のソケットと同じアドレスファミリを優先して繋ぐ。
