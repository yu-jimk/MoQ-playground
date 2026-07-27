// MoQ ライブ配信の統合フロントエンド(React)。
// 配信 / 視聴 / チャット / 参加者一覧を 1 画面で行う。
import { useRef, useState } from "react";
import "@moq/publish/element";
import "@moq/watch/element";
import type * as Moq from "@moq/net";
import {
  connect,
  loadCertHash,
  publishChat,
  runDiscovery,
  sendChat,
  sendPing,
  webtransportProps,
  PING_INTERVAL_MS,
  PRESENCE_TIMEOUT_MS,
  type Established,
  type WebTransportProps,
} from "./moq";
import { MoqPublish } from "./components/MoqPublish";
import { MoqWatch } from "./components/MoqWatch";

type Joined = { url: URL; myName: string; webtransport: WebTransportProps };
type Message = { user: string; text: string; self: boolean };

export default function App() {
  const [url, setUrl] = useState("https://localhost:4443/");
  const [name, setName] = useState("");
  const [joined, setJoined] = useState<Joined | null>(null);
  const [live, setLive] = useState(false);
  const [remotes, setRemotes] = useState<string[]>([]); // 配信中の他人(メディア broadcast パス)
  const [members, setMembers] = useState<string[]>([]); // チャット参加者(自分含む)
  const [messages, setMessages] = useState<Message[]>([]);
  const [chatText, setChatText] = useState("");
  const [status, setStatus] = useState("未接続");

  // React の再レンダに載せない実体。
  const sessionRef = useRef<Established | null>(null);
  const chatTrackRef = useRef<Moq.Track.Producer | null>(null);
  // 参加者ごとの最終受信時刻(ping/メッセージ)。プレゼンス管理に使う。
  const lastSeenRef = useRef<Map<string, number>>(new Map());
  const pingTimerRef = useRef<number | null>(null);
  const pruneTimerRef = useRef<number | null>(null);

  function addMessage(user: string, text: string, self: boolean) {
    setMessages((m) => [...m, { user, text, self }]);
  }

  // 誰かからの ping/メッセージを受けたら生存時刻を更新し、新規なら参加者に追加。
  function markSeen(user: string) {
    lastSeenRef.current.set(user, Date.now());
    setMembers((cur) => (cur.includes(user) ? cur : [...cur, user]));
  }

  async function join() {
    if (joined) return;
    try {
      const myName = name.trim() || `user-${Math.floor(Math.random() * 1000)}`;
      setName(myName);
      const u = new URL(url.trim());

      setStatus("証明書ハッシュを取得中…");
      const webtransport = webtransportProps(await loadCertHash());

      setStatus("接続中…");
      const session = await connect(u, webtransport);
      const { track: chatTrack, path: myChatPath } = publishChat(session, myName);
      sessionRef.current = session;
      chatTrackRef.current = chatTrack;

      setJoined({ url: u, myName, webtransport });
      setMembers([myName]); // 自分は常に参加者
      lastSeenRef.current = new Map();
      setStatus(`接続済み (${myName})`);

      runDiscovery(session, myName, myChatPath, {
        onRemoteActive: (p) => setRemotes((r) => (r.includes(p) ? r : [...r, p])),
        onRemoteGone: (p) => setRemotes((r) => r.filter((x) => x !== p)),
        onSeen: (user) => markSeen(user),
        onChat: (user, text) => addMessage(user, text, false),
      }).catch((e) => console.error("discovery:", e));

      // プレゼンス ping を定期送信。
      pingTimerRef.current = window.setInterval(() => {
        if (chatTrackRef.current) sendPing(chatTrackRef.current, myName);
      }, PING_INTERVAL_MS);

      // 一定時間 ping が途絶えた参加者を除去(自分は除く)。
      pruneTimerRef.current = window.setInterval(() => {
        const now = Date.now();
        const seen = lastSeenRef.current;
        setMembers((cur) =>
          cur.filter((m) => m === myName || now - (seen.get(m) ?? now) <= PRESENCE_TIMEOUT_MS),
        );
      }, 2000);
    } catch (e) {
      console.error(e);
      setStatus(`エラー: ${(e as Error).message ?? e}`);
    }
  }

  // 退出: タイマーを止めてセッションを閉じ、UI を初期状態に戻す。
  function leave() {
    if (pingTimerRef.current !== null) window.clearInterval(pingTimerRef.current);
    if (pruneTimerRef.current !== null) window.clearInterval(pruneTimerRef.current);
    pingTimerRef.current = null;
    pruneTimerRef.current = null;
    sessionRef.current?.close();
    sessionRef.current = null;
    chatTrackRef.current = null;
    lastSeenRef.current = new Map();
    setJoined(null);
    setLive(false);
    setRemotes([]);
    setMembers([]);
    setMessages([]);
    setStatus("退出しました");
  }

  function send() {
    const track = chatTrackRef.current;
    if (!track || !joined) return;
    const text = chatText.trim();
    if (!text) return;
    sendChat(track, joined.myName, text);
    addMessage(joined.myName, text, true);
    setChatText("");
  }

  const isLive = (member: string) =>
    joined ? (member === joined.myName ? live : remotes.includes(member)) : false;

  return (
    <>
      <h1>MoQ ライブ配信 — 配信 / 視聴 / チャット統合 (React)</h1>

      <div className="row">
        <label>
          Relay URL{" "}
          <input value={url} onChange={(e) => setUrl(e.target.value)} disabled={!!joined} size={26} />
        </label>
        <label>
          名前{" "}
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={!!joined}
            placeholder="alice"
            size={10}
          />
        </label>
        {!joined ? (
          <button onClick={join}>参加</button>
        ) : (
          <button onClick={leave}>退出</button>
        )}
        {joined &&
          (!live ? (
            <button onClick={() => setLive(true)}>配信開始</button>
          ) : (
            <button onClick={() => setLive(false)}>配信終了</button>
          ))}
        <span className="status">{status}</span>
      </div>

      <div className="layout">
        <div>
          <h2>自分の配信プレビュー</h2>
          {joined && live ? (
            <div className="tile">
              <MoqPublish url={joined.url} name={joined.myName} webtransport={joined.webtransport} />
              <figcaption>配信中: {joined.myName}</figcaption>
            </div>
          ) : (
            <p className="muted">「配信開始」でカメラ/マイクを配信します。</p>
          )}

          <h2>視聴中の配信</h2>
          <div className="videos">
            {joined &&
              remotes.map((path) => (
                <MoqWatch key={path} url={joined.url} name={path} webtransport={joined.webtransport} />
              ))}
            {remotes.length === 0 && <p className="muted">他の配信者はまだいません。</p>}
          </div>
        </div>

        <div>
          <h2>参加メンバー ({members.length})</h2>
          <ul className="members">
            {members.map((m) => (
              <li key={m}>
                {m}
                {m === joined?.myName && <span className="tag">あなた</span>}
                {isLive(m) && <span className="tag live">配信中</span>}
              </li>
            ))}
            {members.length === 0 && <li className="muted">未接続</li>}
          </ul>

          <h2>チャット</h2>
          <div className="chat-log">
            {messages.map((m, i) => (
              <div className="msg" key={i}>
                <span className={"user" + (m.self ? " self" : "")}>{m.user}: </span>
                {m.text}
              </div>
            ))}
          </div>
          <div className="row chat-input">
            <input
              value={chatText}
              onChange={(e) => setChatText(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && send()}
              placeholder="メッセージ"
              disabled={!joined}
            />
            <button onClick={send} disabled={!joined}>
              送信
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
