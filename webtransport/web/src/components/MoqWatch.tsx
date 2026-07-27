// <moq-watch> の React ラッパ。announce で見つけた配信を再生する。
// MoqPublish と同様、ref 経由で証明書ハッシュを注入してから url / name を設定する。
import { useEffect, useRef } from "react";
import type * as Moq from "@moq/net";
import type { WebTransportProps } from "../moq";

type WatchElement = HTMLElement & {
  connection: Moq.Connection.Reload;
  url: URL | string;
  name: string;
};

export function MoqWatch(props: { url: URL; name: string; webtransport: WebTransportProps }) {
  const ref = useRef<WatchElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.connection.webtransport = props.webtransport;
    el.url = props.url;
    el.name = props.name;
  }, [props.url, props.name, props.webtransport]);

  return (
    <figure className="tile">
      <moq-watch ref={ref}>
        <canvas />
      </moq-watch>
      <figcaption>{props.name}</figcaption>
    </figure>
  );
}
