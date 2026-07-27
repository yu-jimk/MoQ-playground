// <moq-publish> の React ラッパ。カメラ/マイクを取得し WebCodecs でエンコードして
// 配信する。証明書ハッシュは属性で渡せないので、ref 経由で connection.webtransport に
// 注入してから url / name を設定する。
import { useEffect, useRef } from "react";
import type * as Moq from "@moq/net";
import type { WebTransportProps } from "../moq";

type PublishElement = HTMLElement & {
  connection: Moq.Connection.Reload;
  url: URL | string;
  name: string;
};

export function MoqPublish(props: { url: URL; name: string; webtransport: WebTransportProps }) {
  const ref = useRef<PublishElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    // webtransport を先に注入してから url を設定すると、その設定で接続が始まる。
    el.connection.webtransport = props.webtransport;
    el.url = props.url;
    el.name = props.name;
  }, [props.url, props.name, props.webtransport]);

  return (
    <moq-publish ref={ref} source="camera">
      <video muted autoPlay playsInline />
    </moq-publish>
  );
}
