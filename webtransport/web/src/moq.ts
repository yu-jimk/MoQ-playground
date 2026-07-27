// MoQ 接続まわりの純ロジック(React に依存しない)。
// UI からはコールバックで状態更新してもらう。
//
// プレゼンス(参加者管理)は「ハートビート方式」で行う:
//   - 各自が自分のチャット track に定期的に ping を送る
//   - 受信側は ping/メッセージの "user" を見て生存時刻を更新し、
//     一定時間途絶えた相手を退室とみなす(App 側のタイマーで除去)
// moq-net の announce は「追加」は届くが「撤回(退室)」が確実に伝わらないため、
// 撤回に依存しないこの方式にしている。チャットのパスはセッションごとに
// ユニークにして、退室→再入室でも新しい announce として検知できるようにする。

import * as Moq from "@moq/net";

/** チャット broadcast の接頭辞。メディア broadcast と名前空間を分ける。 */
export const CHAT_PREFIX = Moq.Path.from("chat");

export type Established = Moq.Connection.Established;
export type WebTransportProps = Moq.Connection.WebTransportProps;

/** ping の送信間隔(ミリ秒)。 */
export const PING_INTERVAL_MS = 3000;
/** この時間 ping が途絶えたら退室とみなす(ミリ秒)。 */
export const PRESENCE_TIMEOUT_MS = 9000;

/** relay が書き出した証明書ハッシュ(hex)を読む。 */
export async function loadCertHash(): Promise<string> {
  const res = await fetch("/cert-hash.hex", { cache: "no-store" });
  if (!res.ok) {
    throw new Error(`cert-hash.hex を取得できません (${res.status})。relay を起動しましたか?`);
  }
  return (await res.text()).trim();
}

/** 証明書ハッシュから WebTransport のピン留めオプションを作る。 */
export function webtransportProps(hashHex: string): WebTransportProps {
  return { serverCertificateHashes: [{ value: hashHex }] };
}

/** relay へ WebTransport で接続する。 */
export function connect(url: URL, webtransport: WebTransportProps): Promise<Established> {
  return Moq.Connection.connect(url, { webtransport });
}

/** 自分のチャット broadcast を publish し、書き込み口と自分のパスを返す。 */
export function publishChat(session: Established, name: string): { track: Moq.Track.Producer; path: Moq.Path.Valid } {
  const broadcast = new Moq.Broadcast.Producer();
  const track = broadcast.createTrack("messages");
  // セッションごとにユニークなパス(chat/<name>/<uuid>)。
  const path = Moq.Path.from("chat", name, crypto.randomUUID());
  session.publish(path, broadcast);
  return { track, path };
}

/** チャットを 1 行送る(web/Rust 共通の {"user","text"} JSON 形式)。 */
export function sendChat(track: Moq.Track.Producer, user: string, text: string): void {
  track.writeString(JSON.stringify({ user, text }));
}

/** 存在確認の ping を送る(web/Rust 共通の {"user","presence":true})。 */
export function sendPing(track: Moq.Track.Producer, user: string): void {
  track.writeString(JSON.stringify({ user, presence: true }));
}

export type DiscoveryHandlers = {
  /** メディア broadcast が現れた(自分以外)。 */
  onRemoteActive: (path: string) => void;
  /** メディア broadcast が消えた。 */
  onRemoteGone: (path: string) => void;
  /** チャット/ping を受信した(生存時刻の更新に使う)。 */
  onSeen: (user: string) => void;
  /** チャットメッセージ本文を受信した。 */
  onChat: (user: string, text: string) => void;
};

/**
 * announce を監視し、チャット broadcast を購読し、メディアの出入りを追う。
 * 参加者名はパスではなく受信メッセージの "user" で判別する。
 * 接続が閉じるまで戻らないので、呼び出し側は await せず投げっぱなしにする。
 */
export async function runDiscovery(
  session: Established,
  myName: string,
  myChatPath: Moq.Path.Valid,
  handlers: DiscoveryHandlers,
): Promise<void> {
  const announced = session.announced();
  for (;;) {
    const ev = await announced.next();
    if (!ev) break;
    const path = ev.path;

    if (Moq.Path.hasPrefix(CHAT_PREFIX, path)) {
      // 自分の broadcast は購読しない。退室は App のタイムアウトで扱う。
      if (path === myChatPath) continue;
      if (ev.active) void subscribeChat(session, path, myName, handlers);
      continue;
    }

    // それ以外 = メディア broadcast(= その人が配信中)。
    if (path === myName) continue;
    if (ev.active) handlers.onRemoteActive(path);
    else handlers.onRemoteGone(path);
  }
}

/**
 * チャット track を購読し、chat とプレゼンス ping を処理する。
 * 参加者は各メッセージの "user" フィールドで識別する(パスに依存しない)。
 * `recvGroup` で到着順に読む(readString の「古い group を捨てる」挙動は使わない)。
 */
async function subscribeChat(
  session: Established,
  path: Moq.Path.Valid,
  myName: string,
  handlers: DiscoveryHandlers,
): Promise<void> {
  const broadcast = session.consume(path);
  const sub = broadcast.subscribe("messages");
  try {
    for (;;) {
      const group = await sub.recvGroup();
      if (group === undefined) break;
      for (;;) {
        const raw = await group.readString();
        if (raw === undefined) break;
        let user: string | undefined;
        let text: string | undefined;
        try {
          const obj = JSON.parse(raw) as { user?: string; text?: string };
          user = obj.user;
          text = obj.text;
        } catch {
          continue;
        }
        if (!user || user === myName) continue; // 自分の echo は無視
        handlers.onSeen(user);
        if (typeof text === "string") handlers.onChat(user, text);
      }
    }
  } finally {
    sub.close();
  }
}
