# ① quic/ — QUIC の stream と datagram

MoQ の前段として、QUIC そのものの 2 つのデータ転送手段を最小構成で触る。

| 転送手段 | 順序保証 | 再送 | 用途 |
| --- | --- | --- | --- |
| **stream**（双方向ストリーム） | あり | あり | 落としたくないデータ。MoQ の frame もこちら |
| **datagram** | なし | なし | 遅れて届くなら要らないデータ |

QUIC は必ず TLS で暗号化されるので、どちらも証明書が必要になる。その用意は
[transport](transport/) にまとめてある（サーバは自己署名証明書をその場で生成し、
クライアントは検証をスキップする）。**ローカル検証専用**。

## クレート

| クレート | バイナリ | 役割 |
| --- | --- | --- |
| [transport](transport/) | — | 証明書生成 / 検証スキップの共有ライブラリ。②③ からも使う |
| [stream](stream/) | `stream-server`, `stream-client` | `open_bi` / `accept_bi` で ping/pong |
| [datagram](datagram/) | `datagram-server`, `datagram-client` | `send_datagram` / `read_datagram` で ping/pong |

## 実行

```bash
# ストリーム版
cargo run -p stream --bin stream-server
cargo run -p stream --bin stream-client     # 別ターミナル

# データグラム版
cargo run -p datagram --bin datagram-server
cargo run -p datagram --bin datagram-client # 別ターミナル
```

## テスト

サーバとクライアントを同一プロセスで起動して往復を検証する。

```bash
cargo test -p stream -p datagram
```
